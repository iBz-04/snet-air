//! Real servers for the engine's tests: a TLS server on 127.0.0.1 with a
//! test certificate authority, and a plain HTTP/1.1 server on 127.0.0.1.
//! Only test code depends on this crate.

pub mod certificates;
pub mod http;
pub mod loopback;
pub mod tls;

/// Errors from setting a server up; tests unwrap them.
pub type SetupError = Box<dyn std::error::Error + Send + Sync>;
