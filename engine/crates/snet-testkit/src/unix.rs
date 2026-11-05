use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::UnixListener;

use crate::SetupError;
use crate::http::{ReceivedRequest, read_request};

/// How the Unix-socket server answers one request.
pub enum UnixReply {
    Bytes(Vec<u8>),
    After(Duration, Vec<u8>),
    Stall,
}

pub type UnixHandler = Arc<dyn Fn(&ReceivedRequest) -> UnixReply + Send + Sync>;

#[derive(Default)]
pub struct UnixObservations {
    pub connections: AtomicUsize,
    pub bytes_received: AtomicUsize,
    pub in_flight: AtomicUsize,
    pub most_in_flight: AtomicUsize,
    pub requests: Mutex<Vec<ReceivedRequest>>,
}

impl UnixObservations {
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

/// An HTTP/1.1 server on a Unix socket in a private directory, run by the
/// test under its own account, one request per connection.
pub struct UnixServer {
    _directory: tempfile::TempDir,
    pub socket: PathBuf,
    pub observations: Arc<UnixObservations>,
}

impl UnixServer {
    pub async fn start(handler: UnixHandler) -> Result<Self, SetupError> {
        let directory = tempfile::tempdir()?;
        std::fs::set_permissions(directory.path(), PermissionsExt::from_mode(0o700))?;
        let socket = directory.path().join("a.sock");
        let listener = UnixListener::bind(&socket)?;
        let observations = Arc::new(UnixObservations::default());
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
                        UnixReply::Bytes(bytes) => bytes,
                        UnixReply::After(delay, bytes) => {
                            tokio::time::sleep(delay).await;
                            bytes
                        }
                        UnixReply::Stall => {
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
            _directory: directory,
            socket,
            observations,
        })
    }
}
