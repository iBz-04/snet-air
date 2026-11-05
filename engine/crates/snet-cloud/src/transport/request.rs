use bytes::Bytes;
use http::header::{CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, HOST};
use http::{HeaderMap, HeaderName, HeaderValue, Method, Request};
use http_body_util::Full;

use crate::config::{EndpointConfig, KeyHeader, WireSettings};
use crate::errors::Failure;
use crate::secret::ApiKey;
use crate::wire;

/// One request ready to send: its path, its exact headers and the
/// body that is also handed to the recorder.
#[derive(Debug)]
pub(crate) struct PreparedRequest {
    pub(crate) path: String,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Bytes,
}

impl PreparedRequest {
    pub(crate) fn new(config: &EndpointConfig, key: &ApiKey, body: Bytes) -> Result<Self, Failure> {
        let invalid = |reason: &str| Failure::RequestInvalid {
            reason: reason.to_string(),
        };
        let mut headers = HeaderMap::new();
        let host = HeaderValue::from_str(config.base_url.authority())
            .map_err(|_| invalid("the host cannot be sent as a header value"))?;
        headers.insert(HOST, host);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(CONTENT_LENGTH, HeaderValue::from(body.len()));

        let key_header = match config.wire {
            WireSettings::OpenaiChat(settings) => settings.key_header,
            WireSettings::AnthropicMessages => KeyHeader::AuthorizationBearer,
        };
        let (key_name, key_value) = key.header(key_header)?;
        headers.insert(HeaderName::from_static(key_name), key_value);

        if config.wire == WireSettings::AnthropicMessages {
            headers.insert(
                HeaderName::from_static("anthropic-version"),
                HeaderValue::from_static(wire::anthropic_messages::API_VERSION),
            );
            // Not secret: it names the destination and cannot authenticate,
            // so it is recorded with the other plain headers.
            if let Some(workspace) = &config.anthropic_workspace_id {
                let value = HeaderValue::from_str(workspace)
                    .map_err(|_| invalid("the workspace ID cannot be sent as a header value"))?;
                headers.insert(HeaderName::from_static("anthropic-workspace-id"), value);
            }
        }
        headers.insert(CONNECTION, HeaderValue::from_static("close"));

        Ok(Self {
            path: config
                .base_url
                .path_for(wire::endpoint_path(config.wire.format())),
            headers,
            body,
        })
    }

    /// The headers that hold no secret, for the outbound record.
    pub(crate) fn recordable_headers(&self) -> Vec<(String, String)> {
        self.headers
            .iter()
            .filter(|(_, value)| !value.is_sensitive())
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect()
    }

    /// The number of bytes HTTP/1.1 puts on the wire for this request. hyper
    /// adds no header of its own when `host`, `content-length` and
    /// `connection` are set, which the header test checks.
    pub(crate) fn wire_length(&self) -> u64 {
        let request_line = "POST ".len() + self.path.len() + " HTTP/1.1\r\n".len();
        let header_lines: usize = self
            .headers
            .iter()
            .map(|(name, value)| name.as_str().len() + ": ".len() + value.len() + "\r\n".len())
            .sum();
        (request_line + header_lines + "\r\n".len() + self.body.len()) as u64
    }

    pub(crate) fn into_http(self) -> Result<Request<Full<Bytes>>, Failure> {
        let uri = self.path.parse().map_err(|_| Failure::RequestInvalid {
            reason: "the request path is not a valid URI".to_string(),
        })?;
        let mut request = Request::new(Full::new(self.body));
        *request.method_mut() = Method::POST;
        *request.uri_mut() = uri;
        *request.headers_mut() = self.headers;
        Ok(request)
    }
}
