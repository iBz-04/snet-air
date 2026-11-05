//! What a reply cut short reports, so the user can see why.

use std::fmt;

/// Why the provider stopped a reply before it was finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationStop {
    /// Anthropic `max_tokens`: the output cap.
    OutputCap,
    /// Anthropic `model_context_window_exceeded`: the model's context window.
    ContextWindow,
    /// OpenAI-style `length`: the output cap or the context window.
    Length,
}

/// A reply cut short, with the counts its usage stated. It holds no reply
/// text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TruncatedReply {
    pub stop: TruncationStop,
    pub output_tokens: u128,
    /// Thinking (Anthropic) or reasoning (OpenAI-style) tokens, where the
    /// reply states them; they are inside `output_tokens`.
    pub thinking_tokens: Option<u128>,
    pub output_cap: u64,
}

impl fmt::Display for TruncatedReply {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            stop,
            output_tokens,
            thinking_tokens,
            output_cap,
        } = *self;
        let thinking = |word: &str| match thinking_tokens {
            Some(tokens) => format!(", about {tokens} of them {word}"),
            None => String::new(),
        };
        match stop {
            TruncationStop::OutputCap => write!(
                formatter,
                "the reply stopped at the output cap of {output_cap} tokens after {output_tokens} output tokens{}; lower `effort` or raise `max_output_tokens`",
                thinking("thinking")
            ),
            TruncationStop::ContextWindow => write!(
                formatter,
                "the request and its reply filled the model's context window after {output_tokens} output tokens{}; send less (fewer rounds or shorter passages) or choose a model with a larger window",
                thinking("thinking")
            ),
            TruncationStop::Length => write!(
                formatter,
                "the reply stopped at the output cap of {output_cap} tokens or at the model's context window after {output_tokens} output tokens{}; lower `effort` or raise `max_output_tokens`, or send less",
                thinking("reasoning")
            ),
        }
    }
}
