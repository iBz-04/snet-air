use serde::Serialize;
use snet_wire::{ConversationMessage, ConversationRole, ReplySchema};

use crate::config::LocalConfig;
use crate::errors::LocalError;

/// Bytes added per message to bound the chat template's own tokens.
pub(crate) const TEMPLATE_BYTES_PER_MESSAGE: u64 = 32;

/// One structured question for the local model.
#[derive(Clone)]
pub struct LocalRequest {
    pub instructions: String,
    pub conversation: Vec<ConversationMessage>,
    pub reply_schema: ReplySchema,
    pub max_output_tokens: u64,
}

impl std::fmt::Debug for LocalRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "LocalRequest({} messages, schema `{}`, {} output tokens)",
            self.conversation.len() + 1,
            self.reply_schema.name(),
            self.max_output_tokens
        )
    }
}

#[derive(Serialize)]
struct Body<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
    stream: bool,
    /// A thinking model would spend the output cap on reasoning the
    /// protocols never read; Ollama ignores `false` for other models.
    think: bool,
    /// Ollama constrains the answer to this JSON schema.
    format: &'a serde_json::Value,
    options: Options,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Serialize)]
struct Options {
    temperature: u8,
    num_predict: u64,
    num_ctx: u64,
}

fn json_depth(value: &serde_json::Value) -> u64 {
    match value {
        serde_json::Value::Array(items) => 1 + items.iter().map(json_depth).max().unwrap_or(0),
        serde_json::Value::Object(fields) => 1 + fields.values().map(json_depth).max().unwrap_or(0),
        _ => 1,
    }
}

/// The checks every backend runs before the model sees a request.
pub(crate) fn check_request(
    config: &LocalConfig,
    request: &LocalRequest,
) -> Result<(), LocalError> {
    let invalid = |reason: &str| LocalError::RequestInvalid {
        reason: reason.to_string(),
    };
    if request.instructions.trim().is_empty() {
        return Err(invalid("the instructions are empty"));
    }
    if request.conversation.is_empty() {
        return Err(invalid("the conversation has no message"));
    }
    if request
        .conversation
        .iter()
        .any(|message| message.text.trim().is_empty())
    {
        return Err(invalid("a conversation message is empty"));
    }
    if request.max_output_tokens == 0 || request.max_output_tokens > config.max_output_tokens {
        return Err(invalid(
            "the output token cap is outside 1 to the configured cap",
        ));
    }
    let texts = std::iter::once(request.instructions.as_str()).chain(
        request
            .conversation
            .iter()
            .map(|message| message.text.as_str()),
    );
    let mut prompt_bound = 0u64;
    for text in texts {
        if config
            .control_tokens
            .iter()
            .any(|token| text.contains(token.as_str()))
        {
            return Err(LocalError::ControlToken);
        }
        prompt_bound += text.len() as u64 + TEMPLATE_BYTES_PER_MESSAGE;
    }
    if prompt_bound + request.max_output_tokens > config.context_tokens {
        return Err(LocalError::ContextExceeded {
            prompt_bound,
            output_cap: request.max_output_tokens,
            context: config.context_tokens,
        });
    }
    let schema = request.reply_schema.as_json();
    let schema_bytes = serde_json::to_vec(schema)
        .map_err(|error| invalid(&error.to_string()))?
        .len() as u64;
    if schema_bytes > config.max_schema_bytes {
        return Err(invalid("the schema is larger than the configured bound"));
    }
    if json_depth(schema) > config.max_schema_depth {
        return Err(invalid("the schema is deeper than the configured bound"));
    }
    Ok(())
}

/// The Ollama chat body of a request that passed `check_request`.
pub(crate) fn build_body(
    config: &LocalConfig,
    request: &LocalRequest,
) -> Result<Vec<u8>, LocalError> {
    check_request(config, request)?;
    let invalid = |reason: &str| LocalError::RequestInvalid {
        reason: reason.to_string(),
    };
    let schema = request.reply_schema.as_json();
    let mut messages = vec![Message {
        role: "system",
        content: &request.instructions,
    }];
    for message in &request.conversation {
        messages.push(Message {
            role: match message.role {
                ConversationRole::User => "user",
                ConversationRole::Assistant => "assistant",
            },
            content: &message.text,
        });
    }
    let body = serde_json::to_vec(&Body {
        model: &config.model,
        messages,
        stream: false,
        think: false,
        format: schema,
        options: Options {
            temperature: 0,
            num_predict: request.max_output_tokens,
            num_ctx: config.context_tokens,
        },
    })
    .map_err(|error| invalid(&error.to_string()))?;
    if body.len() as u64 > config.request_bytes_cap {
        return Err(invalid("the body is larger than the request cap"));
    }
    Ok(body)
}

#[cfg(test)]
mod tests;
