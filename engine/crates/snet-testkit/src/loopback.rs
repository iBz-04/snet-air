use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

use crate::SetupError;
use crate::http::{ReceivedRequest, read_request};

/// How the loopback server answers one request.
pub enum LoopbackReply {
    Bytes(Vec<u8>),
    After(Duration, Vec<u8>),
    Stall,
}

pub type LoopbackHandler = Arc<dyn Fn(&ReceivedRequest) -> LoopbackReply + Send + Sync>;

#[derive(Default)]
pub struct LoopbackObservations {
    pub connections: AtomicUsize,
    pub bytes_received: AtomicUsize,
    pub in_flight: AtomicUsize,
    pub most_in_flight: AtomicUsize,
    pub requests: Mutex<Vec<ReceivedRequest>>,
}

impl LoopbackObservations {
    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    pub fn requests(&self) -> Vec<ReceivedRequest> {
        self.requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// A plain HTTP/1.1 server on 127.0.0.1 at a free port, one request per
/// connection, standing in for a local model server.
pub struct LoopbackServer {
    pub address: SocketAddr,
    pub observations: Arc<LoopbackObservations>,
}

impl LoopbackServer {
    pub async fn start(handler: LoopbackHandler) -> Result<Self, SetupError> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let observations = Arc::new(LoopbackObservations::default());
        let shared = Arc::clone(&observations);
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                shared.connections.fetch_add(1, Ordering::SeqCst);
                let observations = Arc::clone(&shared);
                let handler = Arc::clone(&handler);
                tokio::spawn(async move {
                    let Some(request) =
                        read_request(&mut stream, &observations.bytes_received).await
                    else {
                        return;
                    };
                    let now = observations.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    observations.most_in_flight.fetch_max(now, Ordering::SeqCst);
                    observations
                        .requests
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(request.clone());
                    let bytes = match handler(&request) {
                        LoopbackReply::Bytes(bytes) => bytes,
                        LoopbackReply::After(delay, bytes) => {
                            tokio::time::sleep(delay).await;
                            bytes
                        }
                        LoopbackReply::Stall => {
                            std::future::pending::<()>().await;
                            Vec::new()
                        }
                    };
                    observations.in_flight.fetch_sub(1, Ordering::SeqCst);
                    let _client_may_have_left = stream.write_all(&bytes).await;
                });
            }
        });
        Ok(Self {
            address,
            observations,
        })
    }
}
