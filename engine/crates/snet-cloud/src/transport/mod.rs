mod reply;
mod request;

use std::error::Error as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use hyper::client::conn::http1;
use rustls::ClientConfig;
use tokio::net::TcpStream;
use tokio::time::Instant;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

pub(crate) use request::PreparedRequest;

use crate::config::EndpointConfig;
use crate::errors::Failure;
use crate::io::{HyperIo, ReplyHeadTooLarge};
use crate::record::BytesWritten;
use crate::resolve;
use crate::switch::{self, CloudSwitch};

const MAX_REPLY_HEADERS: usize = 64;
const MAX_REPLY_HEAD_BYTES: usize = 65_536;

/// What an exchange got through, kept outside the exchange future so that a
/// timeout or a switch abort still leaves it for the outcome record.
#[derive(Debug)]
pub(crate) struct ExchangeProgress {
    bytes_written: Arc<AtomicU64>,
    full_request_length: u64,
    pub(crate) status: Option<u16>,
    pub(crate) request_id: Option<String>,
    pub(crate) reply_body: Vec<u8>,
}

impl ExchangeProgress {
    pub(crate) fn new(request: &PreparedRequest) -> Self {
        Self {
            bytes_written: Arc::new(AtomicU64::new(0)),
            full_request_length: request.wire_length(),
            status: None,
            request_id: None,
            reply_body: Vec::new(),
        }
    }

    pub(crate) fn bytes_written(&self) -> BytesWritten {
        match self.bytes_written.load(Ordering::SeqCst) {
            0 => BytesWritten::None,
            written if written >= self.full_request_length => BytesWritten::All,
            _ => BytesWritten::Some,
        }
    }
}

pub(crate) struct Exchange<'a> {
    pub(crate) config: &'a EndpointConfig,
    pub(crate) tls: Arc<ClientConfig>,
    pub(crate) switch: &'a CloudSwitch,
    pub(crate) request: PreparedRequest,
    pub(crate) request_id_header: &'static str,
}

/// One HTTP/1.1 exchange on a new connection, bounded by the connect and
/// total timeouts and aborted the moment the switch turns off
///.
pub(crate) async fn run(
    exchange: Exchange<'_>,
    progress: &mut ExchangeProgress,
) -> Result<(), Failure> {
    let started = Instant::now();
    let connect_deadline = deadline(started, exchange.config.connect_timeout)?;
    let total_deadline = deadline(started, exchange.config.total_timeout)?;

    let mut switch_watch = exchange.switch.watch();
    switch_watch.mark_unchanged();
    if !exchange.switch.allows_sending().await {
        return Err(Failure::SwitchOff);
    }

    tokio::select! {
        biased;
        () = switch::turned_off(&mut switch_watch) => Err(Failure::SwitchAborted),
        () = tokio::time::sleep_until(total_deadline) => Err(Failure::TotalTimeout),
        result = exchange_steps(exchange, progress, connect_deadline) => result,
    }
}

fn deadline(started: Instant, timeout: std::time::Duration) -> Result<Instant, Failure> {
    started.checked_add(timeout).ok_or(Failure::RequestInvalid {
        reason: "a timeout does not fit the clock".to_string(),
    })
}

async fn exchange_steps(
    exchange: Exchange<'_>,
    progress: &mut ExchangeProgress,
    connect_deadline: Instant,
) -> Result<(), Failure> {
    let config = exchange.config;
    let stream = tokio::time::timeout_at(connect_deadline, connect(config, exchange.tls))
        .await
        .map_err(|_| Failure::ConnectTimeout)??;

    // The last check before hyper writes the first request byte.
    if !exchange.switch.allows_sending().await {
        return Err(Failure::SwitchOff);
    }

    let io = HyperIo::new(
        stream,
        Arc::clone(&progress.bytes_written),
        MAX_REPLY_HEAD_BYTES,
    );
    let (mut sender, connection) = http1::Builder::new()
        .max_headers(MAX_REPLY_HEADERS)
        .max_buf_size(MAX_REPLY_HEAD_BYTES)
        .handshake(io)
        .await
        .map_err(classify_hyper_error)?;
    let http_request = exchange.request.into_http()?;

    let reply_bytes_cap = config.reply_bytes_cap;
    let request_id_header = exchange.request_id_header;
    let send_and_read = async {
        let response = sender
            .send_request(http_request)
            .await
            .map_err(classify_hyper_error)?;
        reply::read_reply(response, reply_bytes_cap, request_id_header, progress).await
    };

    // hyper's connection driver and the request run together in this task;
    // dropping both closes the connection.
    tokio::pin!(connection);
    tokio::pin!(send_and_read);
    tokio::select! {
        biased;
        result = &mut send_and_read => result,
        ended = &mut connection => match ended {
            Err(error) => Err(classify_hyper_error(error)),
            Ok(()) => send_and_read.as_mut().await,
        },
    }
}

async fn connect(
    config: &EndpointConfig,
    tls: Arc<ClientConfig>,
) -> Result<TlsStream<TcpStream>, Failure> {
    let host = config.base_url.host();
    let addresses =
        resolve::permitted_addresses(host, config.base_url.port(), config.allow_private_addresses)
            .await
            .map_err(|error| Failure::Resolve {
                host: host.to_string(),
                cause: error.to_string(),
            })?;

    let mut last_error = None;
    let mut connected = None;
    for address in addresses {
        match TcpStream::connect(address).await {
            Ok(stream) => {
                connected = Some(stream);
                break;
            }
            Err(error) => last_error = Some(error),
        }
    }
    let tcp = match (connected, last_error) {
        (Some(stream), _) => stream,
        (None, Some(error)) => {
            return Err(Failure::Connect {
                cause: error.to_string(),
            });
        }
        (None, None) => {
            return Err(Failure::NoPermittedAddress {
                host: host.to_string(),
            });
        }
    };
    tcp.set_nodelay(true).map_err(|error| Failure::Connect {
        cause: error.to_string(),
    })?;

    TlsConnector::from(tls)
        .connect(config.base_url.server_name().clone(), tcp)
        .await
        .map_err(|error| Failure::Tls {
            cause: error.to_string(),
        })
}

/// hyper reports a reply head over `max_headers` as a parse error; telling
/// "too large" apart needs its `server` feature, which is banned, so every
/// parse error of the reply head is refused alike. The exact byte limit is
/// enforced by our IO adapter and arrives as its own IO error.
fn classify_hyper_error(error: hyper::Error) -> Failure {
    let source = error.source();
    let cause = match source {
        Some(source) => format!("{error}: {source}"),
        None => error.to_string(),
    };
    let head_too_large = source
        .and_then(|source| source.downcast_ref::<std::io::Error>())
        .and_then(|io_error| io_error.get_ref())
        .is_some_and(|inner| inner.is::<ReplyHeadTooLarge>());
    if error.is_parse() || head_too_large {
        return Failure::ReplyHeadRefused { cause };
    }
    Failure::Http { cause }
}

#[cfg(test)]
mod tests;
