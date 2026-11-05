use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::crypto::aws_lc_rs;
use rustls::server::ServerConfig;
use rustls::{HandshakeKind, NamedGroup, ProtocolVersion};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

use crate::SetupError;
use crate::certificates::ServerIdentity;
use crate::http::{ReceivedRequest, read_request};

/// Answers a request given its position among all requests the server has
/// received, counting from 0; `None` leaves the request unanswered.
pub type Handler = Arc<dyn Fn(usize, &ReceivedRequest) -> Option<Vec<u8>> + Send + Sync>;

/// What the server does once a TLS connection is up.
#[derive(Clone)]
pub enum Behaviour {
    /// Reads the request, writes these raw bytes and closes.
    Respond(Vec<u8>),
    /// Reads the request, waits, then responds.
    RespondAfter(Duration, Vec<u8>),
    /// Reads the request, writes these bytes and then never writes again.
    WriteThenStall(Vec<u8>),
    /// Accepts the TCP connection and never starts TLS.
    NeverHandshake,
    /// Reads the request and writes what the handler returns.
    Handle(Handler),
    /// Reads the request and never answers.
    Stall,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TlsVersion {
    Tls13,
    Tls12Only,
}

#[derive(Debug, Clone)]
pub struct HandshakeFacts {
    pub version: Option<ProtocolVersion>,
    pub group: Option<NamedGroup>,
    pub alpn: Option<Vec<u8>>,
    pub kind: Option<HandshakeKind>,
}

#[derive(Default)]
pub struct Observations {
    pub connections: AtomicUsize,
    pub failed_handshakes: AtomicUsize,
    pub request_bytes: AtomicUsize,
    pub requests: Mutex<Vec<ReceivedRequest>>,
    pub handshakes: Mutex<Vec<HandshakeFacts>>,
}

impl Observations {
    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    pub fn request_bytes(&self) -> usize {
        self.request_bytes.load(Ordering::SeqCst)
    }

    pub fn requests(&self) -> Vec<ReceivedRequest> {
        self.requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn handshakes(&self) -> Vec<HandshakeFacts> {
        self.handshakes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// A TLS server on 127.0.0.1 with one behaviour for every connection.
pub struct TestServer {
    pub port: u16,
    pub observations: Arc<Observations>,
}

impl TestServer {
    pub async fn start(
        identity: ServerIdentity,
        version: TlsVersion,
        behaviour: Behaviour,
    ) -> Result<Self, SetupError> {
        let versions: &[&rustls::SupportedProtocolVersion] = match version {
            TlsVersion::Tls13 => &[&rustls::version::TLS13],
            TlsVersion::Tls12Only => &[&rustls::version::TLS12],
        };
        let mut config =
            ServerConfig::builder_with_provider(Arc::new(aws_lc_rs::default_provider()))
                .with_protocol_versions(versions)?
                .with_no_client_auth()
                .with_single_cert(identity.chain, identity.key)?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let acceptor = TlsAcceptor::from(Arc::new(config));

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let observations = Arc::new(Observations::default());
        let server_observations = Arc::clone(&observations);
        tokio::spawn(async move {
            while let Ok((tcp, _peer)) = listener.accept().await {
                server_observations
                    .connections
                    .fetch_add(1, Ordering::SeqCst);
                let acceptor = acceptor.clone();
                let observations = Arc::clone(&server_observations);
                let behaviour = behaviour.clone();
                tokio::spawn(serve(tcp, acceptor, observations, behaviour));
            }
        });
        Ok(Self { port, observations })
    }

    /// A plain TCP listener that only counts connections, for checking that
    /// nothing reaches it.
    pub async fn counting_listener() -> Result<(u16, Arc<Observations>), SetupError> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let observations = Arc::new(Observations::default());
        let counter = Arc::clone(&observations);
        tokio::spawn(async move {
            while let Ok((_tcp, _peer)) = listener.accept().await {
                counter.connections.fetch_add(1, Ordering::SeqCst);
            }
        });
        Ok((port, observations))
    }
}

async fn serve(
    tcp: TcpStream,
    acceptor: TlsAcceptor,
    observations: Arc<Observations>,
    behaviour: Behaviour,
) {
    if let Behaviour::NeverHandshake = behaviour {
        hold_open_forever(tcp).await;
        return;
    }
    let mut stream = match acceptor.accept(tcp).await {
        Ok(stream) => stream,
        Err(_) => {
            observations
                .failed_handshakes
                .fetch_add(1, Ordering::SeqCst);
            return;
        }
    };
    {
        let connection = stream.get_ref().1;
        let facts = HandshakeFacts {
            version: connection.protocol_version(),
            group: connection
                .negotiated_key_exchange_group()
                .map(|group| group.name()),
            alpn: connection.alpn_protocol().map(<[u8]>::to_vec),
            kind: connection.handshake_kind(),
        };
        observations
            .handshakes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(facts);
    }
    let Some(request) = read_request(&mut stream, &observations.request_bytes).await else {
        return;
    };
    let position = {
        let mut requests = observations
            .requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        requests.push(request.clone());
        requests.len() - 1
    };

    let (delay, bytes, stall_after) = match behaviour {
        Behaviour::Respond(bytes) => (None, bytes, false),
        Behaviour::RespondAfter(delay, bytes) => (Some(delay), bytes, false),
        Behaviour::WriteThenStall(bytes) => (None, bytes, true),
        Behaviour::Handle(handler) => match handler(position, &request) {
            Some(bytes) => (None, bytes, false),
            None => {
                std::future::pending::<()>().await;
                return;
            }
        },
        Behaviour::Stall => {
            std::future::pending::<()>().await;
            return;
        }
        Behaviour::NeverHandshake => return,
    };
    if let Some(delay) = delay {
        tokio::time::sleep(delay).await;
    }
    let _ignored_if_client_left = stream.write_all(&bytes).await;
    if stall_after {
        let _ignored_if_client_left = stream.flush().await;
        std::future::pending::<()>().await;
    }
    let _ignored_if_client_left = stream.shutdown().await;
}

async fn hold_open_forever(_tcp: TcpStream) {
    std::future::pending::<()>().await;
}
