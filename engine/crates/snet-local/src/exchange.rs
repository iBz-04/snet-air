use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use bytes::Bytes;
use http::{Method, Request};
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use snet_wire::io::HyperIo;
use tokio::net::TcpStream;

use crate::errors::LocalError;
use crate::reply::RawReply;

const MAX_REPLY_HEADERS: usize = 64;
const MAX_REPLY_HEAD_BYTES: usize = 65_536;

fn http_error(error: hyper::Error) -> LocalError {
    if error.is_parse() {
        return LocalError::ReplyHeadRefused;
    }
    let head_too_large = std::error::Error::source(&error)
        .and_then(|source| source.downcast_ref::<std::io::Error>())
        .and_then(|io_error| io_error.get_ref())
        .is_some_and(|inner| inner.is::<snet_wire::io::ReplyHeadTooLarge>());
    if head_too_large {
        return LocalError::ReplyHeadRefused;
    }
    LocalError::Http {
        cause: error.to_string(),
    }
}

/// One HTTP/1.1 exchange on a new loopback connection.
pub(crate) async fn exchange(
    address: SocketAddr,
    method: Method,
    path: &str,
    body: Option<Vec<u8>>,
    reply_cap: u64,
) -> Result<RawReply, LocalError> {
    let stream = TcpStream::connect(address)
        .await
        .map_err(|error| LocalError::Connect {
            address,
            cause: error.to_string(),
        })?;
    let io = HyperIo::new(stream, Arc::new(AtomicU64::new(0)), MAX_REPLY_HEAD_BYTES);
    let (mut sender, connection) = http1::Builder::new()
        .max_headers(MAX_REPLY_HEADERS)
        .max_buf_size(MAX_REPLY_HEAD_BYTES)
        .handshake(io)
        .await
        .map_err(http_error)?;
    let mut request = Request::new(Full::new(Bytes::from(body.unwrap_or_default())));
    *request.method_mut() = method.clone();
    *request.uri_mut() = path.parse().map_err(|_| LocalError::RequestInvalid {
        reason: "the path is not a URI".to_string(),
    })?;
    let host = http::HeaderValue::from_str(&address.to_string()).map_err(|_| {
        LocalError::RequestInvalid {
            reason: "the address cannot be a host header".to_string(),
        }
    })?;
    let headers = request.headers_mut();
    headers.insert(http::header::HOST, host);
    headers.insert(
        http::header::CONNECTION,
        http::HeaderValue::from_static("close"),
    );
    if method == Method::POST {
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );
    }
    let send_and_read = async {
        let response = sender.send_request(request).await.map_err(http_error)?;
        let header = |name: http::header::HeaderName| {
            response
                .headers()
                .get(name)
                .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        };
        let status = response.status().as_u16();
        let content_type = header(http::header::CONTENT_TYPE);
        let content_encoding = header(http::header::CONTENT_ENCODING);
        let mut body = response.into_body();
        let mut collected = Vec::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(http_error)?;
            let Ok(data) = frame.into_data() else {
                continue;
            };
            if collected.len() as u64 + data.len() as u64 > reply_cap {
                return Err(LocalError::ReplyTooLarge { cap: reply_cap });
            }
            collected.extend_from_slice(&data);
        }
        Ok(RawReply {
            status,
            content_type,
            content_encoding,
            body: collected,
        })
    };
    tokio::pin!(connection);
    tokio::pin!(send_and_read);
    tokio::select! {
        biased;
        result = &mut send_and_read => result,
        ended = &mut connection => match ended {
            Err(error) => Err(http_error(error)),
            Ok(()) => send_and_read.as_mut().await,
        },
    }
}
