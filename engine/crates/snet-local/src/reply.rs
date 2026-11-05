use serde::Deserialize;
use snet_wire::json::{describe_json_error, parse_strict};
use snet_wire::{JsonObject, JsonValue, ReplySchema};

use crate::errors::LocalError;

const MAX_SERVER_MESSAGE_CHARACTERS: usize = 200;

pub(crate) struct RawReply {
    pub(crate) status: u16,
    pub(crate) content_type: Option<String>,
    pub(crate) content_encoding: Option<String>,
    pub(crate) body: Vec<u8>,
}

/// The model Ollama lists under the requested name, with the digest that
/// names its exact weights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedModel {
    pub name: String,
    pub digest: String,
}

#[derive(Clone)]
pub struct LocalAnswer {
    pub object: JsonObject,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub model: ListedModel,
}

impl std::fmt::Debug for LocalAnswer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "LocalAnswer({} fields, {} prompt and {} completion tokens, {})",
            self.object.len(),
            self.prompt_tokens,
            self.completion_tokens,
            self.model.name,
        )
    }
}

/// Ollama's error replies carry `{"error": "..."}`. Its text names the
/// model or the fault, never the conversation, and is cut to one short line.
pub(crate) fn server_error(reply: &RawReply) -> LocalError {
    let message = match parse_strict(&reply.body) {
        Ok(JsonValue::Object(root)) => match root.get("error") {
            Some(JsonValue::String(text)) => Some(
                text.chars()
                    .filter(|character| !character.is_control())
                    .take(MAX_SERVER_MESSAGE_CHARACTERS)
                    .collect(),
            ),
            _ => None,
        },
        _ => None,
    };
    LocalError::Server {
        status: reply.status,
        message,
    }
}

fn check_json_reply(reply: &RawReply) -> Result<(), LocalError> {
    if reply.status != 200 {
        return Err(server_error(reply));
    }
    let is_json = reply.content_type.as_deref().is_some_and(|value| {
        value
            .split(';')
            .next()
            .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"))
    });
    if !is_json {
        return Err(LocalError::malformed(
            "the content type is not application/json",
        ));
    }
    if reply
        .content_encoding
        .as_deref()
        .is_some_and(|value| !value.trim().eq_ignore_ascii_case("identity"))
    {
        return Err(LocalError::malformed("the reply is encoded"));
    }
    parse_strict(&reply.body)
        .map_err(|reason| LocalError::malformed(format!("the body: {reason}")))?;
    Ok(())
}

#[derive(Deserialize)]
struct TagList {
    models: Vec<TagEntry>,
}

#[derive(Deserialize)]
struct TagEntry {
    name: String,
    digest: String,
}

/// A name without a tag means the `latest` tag, as Ollama reads it.
fn names_match(listed: &str, requested: &str) -> bool {
    listed == requested
        || (!requested.contains(':')
            && listed
                .strip_suffix(":latest")
                .is_some_and(|base| base == requested))
}

pub(crate) fn listed_model(reply: &RawReply, model: &str) -> Result<ListedModel, LocalError> {
    check_json_reply(reply)?;
    let list: TagList = serde_json::from_slice(&reply.body).map_err(|error| {
        LocalError::malformed(format!("the model list: {}", describe_json_error(&error)))
    })?;
    let entry = list
        .models
        .into_iter()
        .find(|entry| names_match(&entry.name, model))
        .ok_or_else(|| LocalError::ModelNotListed {
            model: model.to_string(),
        })?;
    let digest_is_hex = entry.digest.len() == 64
        && entry
            .digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !digest_is_hex {
        return Err(LocalError::malformed(
            "the listed digest is not 64 lower-case hex characters",
        ));
    }
    Ok(ListedModel {
        name: entry.name,
        digest: entry.digest,
    })
}

#[derive(Deserialize)]
struct ChatReply {
    model: String,
    message: ChatMessage,
    done: bool,
    done_reason: Option<String>,
    // Ollama leaves the count out when it is zero, which happens when the
    // whole prompt was already in its cache.
    #[serde(default)]
    prompt_eval_count: u64,
    eval_count: u64,
}

#[derive(Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

pub(crate) fn answer(
    reply: &RawReply,
    model: &ListedModel,
    output_cap: u64,
    schema: &ReplySchema,
) -> Result<LocalAnswer, LocalError> {
    check_json_reply(reply)?;
    let chat: ChatReply = serde_json::from_slice(&reply.body).map_err(|error| {
        LocalError::malformed(format!("the chat reply: {}", describe_json_error(&error)))
    })?;
    if !names_match(&chat.model, &model.name) && !names_match(&model.name, &chat.model) {
        return Err(LocalError::malformed("the answer names another model"));
    }
    if chat.message.role != "assistant" {
        return Err(LocalError::malformed("the answer is not the assistant's"));
    }
    if !chat.done {
        return Err(LocalError::malformed("the reply is not finished"));
    }
    match chat.done_reason.as_deref() {
        Some("stop") => {}
        Some("length") => return Err(LocalError::Truncated),
        _ => {
            return Err(LocalError::malformed(
                "the reply stopped for an unexpected reason",
            ));
        }
    }
    if chat.eval_count > output_cap {
        return Err(LocalError::malformed(
            "the reply used more tokens than the cap",
        ));
    }
    let JsonValue::Object(object) = parse_strict(chat.message.content.as_bytes())
        .map_err(|reason| LocalError::malformed(format!("the answer: {reason}")))?
    else {
        return Err(LocalError::malformed("the answer is not a JSON object"));
    };
    schema
        .check(&object)
        .map_err(|violation| LocalError::ReplyBreaksSchema {
            path: violation.path,
            reason: violation.reason,
        })?;
    Ok(LocalAnswer {
        object,
        prompt_tokens: chat.prompt_eval_count,
        completion_tokens: chat.eval_count,
        model: model.clone(),
    })
}
