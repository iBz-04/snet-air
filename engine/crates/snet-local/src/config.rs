use std::net::{IpAddr, Ipv4Addr, SocketAddr};
#[cfg(feature = "gguf")]
use std::path::{Path, PathBuf};

use crate::errors::ConfigError;

/// Where Ollama listens unless `OLLAMA_HOST` says otherwise.
pub const DEFAULT_OLLAMA_ADDRESS: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_OLLAMA_PORT);
const DEFAULT_OLLAMA_PORT: u16 = 11434;
const MAX_MODEL_LENGTH: usize = 128;

/// Tokens that open or close a turn in the chat templates of common open
/// models. Material holding one could rewrite the conversation the local
/// model sees, so a request holding one is refused.
const CONTROL_TOKENS: &[&str] = &[
    "<|im_start|>",
    "<|im_end|>",
    "<|endoftext|>",
    "<|begin_of_text|>",
    "<|start_header_id|>",
    "<|end_header_id|>",
    "<|eot_id|>",
    "<start_of_turn>",
    "<end_of_turn>",
    "<|system|>",
    "<|user|>",
    "<|assistant|>",
    "<|end|>",
    "[INST]",
    "[/INST]",
];

/// The bounds a caller may change: the context Ollama allocates, the most
/// output tokens one request may ask for, the time one request may take and
/// how many requests may run at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalBounds {
    pub context_tokens: u64,
    pub max_output_tokens: u64,
    pub total_timeout_seconds: u64,
    pub max_concurrent_requests: u64,
}

fn within(field: &'static str, value: u64, minimum: u64, maximum: u64) -> Result<u64, ConfigError> {
    if value < minimum || value > maximum {
        return Err(invalid(
            field,
            format!("{value} is outside {minimum} to {maximum}"),
        ));
    }
    Ok(value)
}

/// What runs the local model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// An Ollama server on this machine.
    Ollama { address: SocketAddr },
    /// A GGUF file run inside this process by quaynor.
    #[cfg(feature = "gguf")]
    Gguf { path: PathBuf },
}

/// The local model and the bounds every request to it keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalConfig {
    pub(crate) backend: Backend,
    pub(crate) model: String,
    pub(crate) context_tokens: u64,
    pub(crate) max_output_tokens: u64,
    pub(crate) request_bytes_cap: u64,
    pub(crate) reply_bytes_cap: u64,
    pub(crate) max_schema_depth: u64,
    pub(crate) max_schema_bytes: u64,
    pub(crate) total_timeout_seconds: u64,
    pub(crate) max_concurrent_requests: u64,
    pub(crate) control_tokens: Vec<String>,
}

fn invalid(field: &'static str, reason: impl Into<String>) -> ConfigError {
    ConfigError::Invalid {
        field,
        reason: reason.into(),
    }
}

impl LocalConfig {
    /// An Ollama model at `ollama_host`, written as `OLLAMA_HOST` takes it,
    /// or at 127.0.0.1:11434 when that is unset.
    pub fn ollama(model: &str, ollama_host: Option<&str>) -> Result<Self, ConfigError> {
        let model_is_plain = !model.is_empty()
            && model.len() <= MAX_MODEL_LENGTH
            && model.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
            });
        if !model_is_plain {
            return Err(invalid(
                "model",
                "it must be 1 to 128 letters, digits, `_`, `-`, `.`, `:` or `/`",
            ));
        }
        let address = ollama_address(ollama_host)?;
        Ok(Self {
            backend: Backend::Ollama { address },
            model: model.to_string(),
            context_tokens: 8_192,
            max_output_tokens: 1_024,
            request_bytes_cap: 1_048_576,
            reply_bytes_cap: 1_048_576,
            max_schema_depth: 16,
            max_schema_bytes: 65_536,
            total_timeout_seconds: 600,
            max_concurrent_requests: 4,
            control_tokens: CONTROL_TOKENS
                .iter()
                .map(|token| token.to_string())
                .collect(),
        })
    }

    pub fn with_bounds(mut self, bounds: LocalBounds) -> Result<Self, ConfigError> {
        self.context_tokens = within("context_tokens", bounds.context_tokens, 512, 131_072)?;
        self.max_output_tokens = within("max_output_tokens", bounds.max_output_tokens, 1, 2_048)?;
        self.total_timeout_seconds = within(
            "total_timeout_seconds",
            bounds.total_timeout_seconds,
            1,
            1_800,
        )?;
        self.max_concurrent_requests = within(
            "max_concurrent_requests",
            bounds.max_concurrent_requests,
            1,
            16,
        )?;
        Ok(self)
    }

    pub fn bounds(&self) -> LocalBounds {
        LocalBounds {
            context_tokens: self.context_tokens,
            max_output_tokens: self.max_output_tokens,
            total_timeout_seconds: self.total_timeout_seconds,
            max_concurrent_requests: self.max_concurrent_requests,
        }
    }

    /// A GGUF file on this machine, run in-process. The path is made
    /// absolute so it can never be read as a remote model address.
    #[cfg(feature = "gguf")]
    pub fn gguf(path: &Path) -> Result<Self, ConfigError> {
        let field = "gguf";
        let has_extension = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"));
        if !has_extension {
            return Err(invalid(
                field,
                format!("`{}` does not end in .gguf", path.display()),
            ));
        }
        let absolute = std::fs::canonicalize(path)
            .map_err(|error| invalid(field, format!("`{}`: {error}", path.display())))?;
        let metadata = std::fs::metadata(&absolute)
            .map_err(|error| invalid(field, format!("`{}`: {error}", path.display())))?;
        if !metadata.is_file() {
            return Err(invalid(
                field,
                format!("`{}` is not a file", path.display()),
            ));
        }
        let model = absolute
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid(field, "the file name is not UTF-8"))?
            .to_string();
        Ok(Self {
            backend: Backend::Gguf { path: absolute },
            model,
            context_tokens: 8_192,
            max_output_tokens: 1_024,
            request_bytes_cap: 1_048_576,
            reply_bytes_cap: 1_048_576,
            max_schema_depth: 16,
            max_schema_bytes: 65_536,
            total_timeout_seconds: 600,
            // The engine runs one chat worker, so requests take turns.
            max_concurrent_requests: 1,
            control_tokens: CONTROL_TOKENS
                .iter()
                .map(|token| token.to_string())
                .collect(),
        })
    }

    pub fn backend(&self) -> &Backend {
        &self.backend
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// How many requests may be in flight at once.
    pub fn max_concurrent_requests(&self) -> u64 {
        self.max_concurrent_requests
    }

    pub fn max_output_tokens(&self) -> u64 {
        self.max_output_tokens
    }
}

/// Where Ollama listens: `OLLAMA_HOST` as Ollama reads it, or
/// 127.0.0.1:11434 when it is unset. Only this machine is accepted.
pub fn ollama_address(ollama_host: Option<&str>) -> Result<SocketAddr, ConfigError> {
    match ollama_host {
        None => Ok(DEFAULT_OLLAMA_ADDRESS),
        Some(text) => loopback_address(text),
    }
}

/// Reads `[http://]host[:port]` and accepts only this machine: the local
/// model reads the material in clear, so it must never be across a network.
/// `0.0.0.0` is how Ollama is told to listen on every interface; a client
/// given it connects to this machine, so it becomes 127.0.0.1.
fn loopback_address(text: &str) -> Result<SocketAddr, ConfigError> {
    let field = "OLLAMA_HOST";
    if text.starts_with("https://") {
        return Err(invalid(field, "Ollama on this machine speaks plain http"));
    }
    let rest = text.strip_prefix("http://").unwrap_or(text);
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.is_empty() || rest.contains('/') || rest.contains('@') {
        return Err(invalid(field, "it must be host or host:port"));
    }
    let (host, port) = split_host_and_port(rest).ok_or_else(|| {
        invalid(
            field,
            "it must be host or host:port, with IPv6 hosts in brackets",
        )
    })?;
    let ip = match host {
        "localhost" | "0.0.0.0" => IpAddr::V4(Ipv4Addr::LOCALHOST),
        "::" => IpAddr::V4(Ipv4Addr::LOCALHOST),
        other => other.parse::<IpAddr>().map_err(|_| {
            invalid(
                field,
                format!("`{other}` is not localhost or an IP address of this machine"),
            )
        })?,
    };
    if !ip.is_loopback() {
        return Err(invalid(
            field,
            format!(
                "{ip} is not this machine; the local model reads your material in clear, so it must run here"
            ),
        ));
    }
    Ok(SocketAddr::new(ip, port.unwrap_or(DEFAULT_OLLAMA_PORT)))
}

fn split_host_and_port(text: &str) -> Option<(&str, Option<u16>)> {
    if let Some(bracketed) = text.strip_prefix('[') {
        let (host, after) = bracketed.split_once(']')?;
        return match after {
            "" => Some((host, None)),
            _ => Some((host, Some(after.strip_prefix(':')?.parse().ok()?))),
        };
    }
    match text.split_once(':') {
        None => Some((text, None)),
        Some((host, port)) if !port.contains(':') => Some((host, Some(port.parse().ok()?))),
        // An unbracketed IPv6 address such as `::1` has no port.
        Some(_) => Some((text, None)),
    }
}

#[cfg(test)]
mod tests;
