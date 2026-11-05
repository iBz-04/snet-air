//! Real servers for the engine's tests: a TLS server on 127.0.0.1 with a
//! test certificate authority, and an HTTP/1.1 server on a Unix socket.
//! Only test code depends on this crate.

pub mod certificates;
pub mod http;
pub mod tls;
#[cfg(unix)]
pub mod unix;

/// Errors from setting a server up; tests unwrap them.
pub type SetupError = Box<dyn std::error::Error + Send + Sync>;
