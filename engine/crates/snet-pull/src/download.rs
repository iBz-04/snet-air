use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use bytes::Bytes;
use http::{Method, Request, StatusCode, Uri, header};
use http_body_util::{BodyExt, Empty};
use hyper::client::conn::http1;
use rustls::crypto::aws_lc_rs::{self, kx_group};
use rustls::{ClientConfig, NoKeyLog, RootCertStore};
use rustls_pki_types::ServerName;
use sha2::{Digest, Sha256};
use snet_wire::io::HyperIo;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::errors::PullError;

const MAX_REDIRECTS: u8 = 5;
const MAX_REPLY_HEAD_BYTES: usize = 64 * 1024;
const MAX_REPLY_HEADERS: usize = 100;
const CONNECT_SECONDS: u64 = 20;
const STALL_SECONDS: u64 = 60;
const USER_AGENT: &str = concat!("snet/", env!("CARGO_PKG_VERSION"));

/// Fetches one file over HTTPS, following redirects only to hosts it
/// allows, and checks its length and SHA-256 as it streams to disk.
pub struct Fetcher {
    tls: Arc<ClientConfig>,
    host_allowed: fn(&str) -> bool,
}

/// The model host and its download hosts.
fn model_host(host: &str) -> bool {
    host == "huggingface.co" || host.ends_with(".hf.co")
}

impl Fetcher {
    /// The fetcher `snet pull` uses: the bundled Mozilla roots and the
    /// model host only.
    pub fn public() -> Result<Self, PullError> {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Self::with_roots(roots, model_host)
    }

    /// A fetcher with its own roots and allowed hosts, for tests against a
    /// local server.
    pub fn with_roots(
        roots: RootCertStore,
        host_allowed: fn(&str) -> bool,
    ) -> Result<Self, PullError> {
        let mut provider = aws_lc_rs::default_provider();
        provider.kx_groups = vec![
            kx_group::X25519MLKEM768,
            kx_group::X25519,
            kx_group::SECP256R1,
            kx_group::SECP384R1,
        ];
        let tls_failed = |error: rustls::Error| PullError::Http {
            cause: format!("the TLS setup failed: {error}"),
        };
        let mut client = ClientConfig::builder_with_provider(Arc::new(provider))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(tls_failed)?
            .with_root_certificates(roots)
            .with_no_client_auth();
        client.alpn_protocols = vec![b"http/1.1".to_vec()];
        client.key_log = Arc::new(NoKeyLog);
        Ok(Self {
            tls: Arc::new(client),
            host_allowed,
        })
    }

    /// Streams `url` into `destination`, calling `progress` with the bytes
    /// written so far. Fails, leaving the partial file for the caller to
    /// remove, when the length or hash differs from what is expected.
    pub async fn download(
        &self,
        url: &str,
        destination: &Path,
        expected_bytes: u64,
        expected_sha256: &str,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), PullError> {
        let mut uri = self.checked_uri(url)?;
        let mut redirects = 0;
        loop {
            let host = uri.host().unwrap_or_default().to_string();
            let port = uri.port_u16().unwrap_or(443);
            let (mut sender, connection) = self.connect(&host, port).await?;
            let connection = tokio::spawn(connection);
            let path = uri
                .path_and_query()
                .map_or("/", |path| path.as_str())
                .to_string();
            let request = Request::builder()
                .method(Method::GET)
                .uri(path)
                .header(header::HOST, &host)
                .header(header::USER_AGENT, USER_AGENT)
                .header(header::ACCEPT, "*/*")
                .body(Empty::<Bytes>::new())
                .map_err(|error| PullError::Http {
                    cause: error.to_string(),
                })?;
            let response = tokio::time::timeout(
                Duration::from_secs(STALL_SECONDS),
                sender.send_request(request),
            )
            .await
            .map_err(|_| PullError::Stalled {
                host: host.clone(),
                seconds: STALL_SECONDS,
            })?
            .map_err(|error| PullError::Http {
                cause: error.to_string(),
            })?;
            let status = response.status();
            if status.is_redirection() {
                connection.abort();
                redirects += 1;
                if redirects > MAX_REDIRECTS {
                    return Err(PullError::TooManyRedirects {
                        limit: MAX_REDIRECTS,
                    });
                }
                let location = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| PullError::Http {
                        cause: "a redirect without a Location".to_string(),
                    })?;
                uri = self.checked_uri(location)?;
                continue;
            }
            if status != StatusCode::OK {
                connection.abort();
                return Err(PullError::Status {
                    status: status.as_u16(),
                });
            }
            let result = write_body(
                response.into_body(),
                &host,
                destination,
                expected_bytes,
                expected_sha256,
                progress,
            )
            .await;
            connection.abort();
            return result;
        }
    }

    /// An absolute https URL whose host this fetcher allows.
    fn checked_uri(&self, text: &str) -> Result<Uri, PullError> {
        let uri: Uri = text.parse().map_err(|_| PullError::Url {
            url: text.to_string(),
        })?;
        let host = match (uri.scheme_str(), uri.host()) {
            (Some("https"), Some(host)) => host,
            _ => {
                return Err(PullError::Url {
                    url: text.to_string(),
                });
            }
        };
        if !(self.host_allowed)(host) {
            return Err(PullError::HostRefused {
                host: host.to_string(),
            });
        }
        Ok(uri)
    }

    async fn connect(
        &self,
        host: &str,
        port: u16,
    ) -> Result<
        (
            http1::SendRequest<Empty<Bytes>>,
            http1::Connection<HyperIo<tokio_rustls::client::TlsStream<TcpStream>>, Empty<Bytes>>,
        ),
        PullError,
    > {
        let connect_failed = |cause: String| PullError::Connect {
            host: host.to_string(),
            cause,
        };
        let tcp = tokio::time::timeout(
            Duration::from_secs(CONNECT_SECONDS),
            TcpStream::connect((host, port)),
        )
        .await
        .map_err(|_| connect_failed("it took too long".to_string()))?
        .map_err(|error| connect_failed(error.to_string()))?;
        let name = ServerName::try_from(host.to_string())
            .map_err(|error| connect_failed(error.to_string()))?;
        let tls = TlsConnector::from(Arc::clone(&self.tls))
            .connect(name, tcp)
            .await
            .map_err(|error| connect_failed(error.to_string()))?;
        let io = HyperIo::new(tls, Arc::new(AtomicU64::new(0)), MAX_REPLY_HEAD_BYTES);
        http1::Builder::new()
            .max_headers(MAX_REPLY_HEADERS)
            .max_buf_size(MAX_REPLY_HEAD_BYTES)
            .handshake(io)
            .await
            .map_err(|error| PullError::Http {
                cause: error.to_string(),
            })
    }
}

async fn write_body(
    mut body: hyper::body::Incoming,
    host: &str,
    destination: &Path,
    expected_bytes: u64,
    expected_sha256: &str,
    progress: &mut dyn FnMut(u64),
) -> Result<(), PullError> {
    let io_failed = |error: std::io::Error| PullError::Io {
        path: destination.display().to_string(),
        cause: error.to_string(),
    };
    let mut file = tokio::fs::File::create(destination)
        .await
        .map_err(io_failed)?;
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(STALL_SECONDS), body.frame())
            .await
            .map_err(|_| PullError::Stalled {
                host: host.to_string(),
                seconds: STALL_SECONDS,
            })?;
        let Some(frame) = frame else { break };
        let frame = frame.map_err(|error| PullError::Http {
            cause: error.to_string(),
        })?;
        let Ok(data) = frame.into_data() else {
            continue;
        };
        written += data.len() as u64;
        if written > expected_bytes {
            return Err(PullError::Length {
                expected: expected_bytes,
                got: written,
            });
        }
        hasher.update(&data);
        file.write_all(&data).await.map_err(io_failed)?;
        progress(written);
    }
    file.flush().await.map_err(io_failed)?;
    file.sync_all().await.map_err(io_failed)?;
    if written != expected_bytes {
        return Err(PullError::Length {
            expected: expected_bytes,
            got: written,
        });
    }
    let got: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if got != expected_sha256 {
        return Err(PullError::Checksum {
            expected: expected_sha256.to_string(),
            got,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_on_the_model_host_is_followed() {
        let fetcher = Fetcher::public().unwrap();
        for allowed in [
            "https://huggingface.co/a/resolve/main/b.gguf",
            "https://us.aws.cdn.hf.co/xet-bridge-us/abc?x=1",
        ] {
            assert!(fetcher.checked_uri(allowed).is_ok(), "{allowed}");
        }
        for refused in [
            "http://huggingface.co/a.gguf",
            "https://evil.example/a.gguf",
            "https://huggingface.co.evil.example/a.gguf",
            "https://hf.co.evil.example/a.gguf",
            "/relative/path",
        ] {
            assert!(fetcher.checked_uri(refused).is_err(), "{refused}");
        }
    }
}
