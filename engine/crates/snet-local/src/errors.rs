use std::net::SocketAddr;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("local model setting `{field}` is invalid: {reason}")]
    Invalid { field: &'static str, reason: String },
}

/// Every message names fields, statuses and the server's error text, and
/// never instruction, message or answer text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LocalError {
    #[error("the request is invalid: {reason}")]
    RequestInvalid { reason: String },
    #[error(
        "the request would not fit the model's context: a prompt bound of {prompt_bound} bytes plus {output_cap} output tokens exceeds {context} tokens"
    )]
    ContextExceeded {
        prompt_bound: u64,
        output_cap: u64,
        context: u64,
    },
    #[error("a message holds one of the model's control tokens")]
    ControlToken,
    #[error("Ollama is not reachable at {address}: {cause}; start it with `ollama serve`")]
    Connect { address: SocketAddr, cause: String },
    #[error("the HTTP exchange with Ollama failed: {cause}")]
    Http { cause: String },
    #[error("the request did not finish within the total timeout")]
    Timeout,
    #[error("the reply head is malformed or exceeds 64 headers or 65 536 bytes")]
    ReplyHeadRefused,
    #[error("the reply body exceeds the cap of {cap} bytes")]
    ReplyTooLarge { cap: u64 },
    #[error("Ollama answered status {status}: {}", .message.as_deref().unwrap_or("no error message"))]
    Server {
        status: u16,
        message: Option<String>,
    },
    #[error("Ollama has no model `{model}`; run `ollama pull {model}`")]
    ModelNotListed { model: String },
    #[error("the answer stopped at the output token cap")]
    Truncated,
    #[error("the reply is malformed: {reason}")]
    MalformedReply { reason: String },
    #[error("the answer breaks the reply schema at {path}: {reason}")]
    ReplyBreaksSchema { path: String, reason: String },
}

impl LocalError {
    pub(crate) fn malformed(reason: impl Into<String>) -> Self {
        Self::MalformedReply {
            reason: reason.into(),
        }
    }
}
