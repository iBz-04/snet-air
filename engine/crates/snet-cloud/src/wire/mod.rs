pub(crate) mod anthropic_messages;
mod error_body;
pub(crate) mod openai_chat;

use bytes::Bytes;

use crate::budget::Charge;
use crate::config::{EndpointConfig, WireFormat, WireSettings};
use crate::errors::Failure;
use crate::json::{self, JsonObject, JsonValue};
use crate::schema::ReplySchema;

pub(crate) use error_body::provider_failure;

pub use crate::request::CloudRequest;
pub(crate) use snet_wire::ConversationRole;

/// Token counts as the provider reported them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportedUsage {
    pub input_tokens: u128,
    pub output_tokens: u128,
    /// The thinking or reasoning share of `output_tokens`, where the reply
    /// states it.
    pub thinking_tokens: Option<u128>,
}

pub(crate) struct ParsedReply {
    pub(crate) usage: ReportedUsage,
    pub(crate) result: Result<AcceptedReply, Failure>,
}

/// The text of a reply and the model the reply names, before the text is
/// parsed and checked.
pub(crate) struct ReplyText {
    pub(crate) text: String,
    pub(crate) model: String,
}

#[derive(Debug)]
pub(crate) struct AcceptedReply {
    pub(crate) object: JsonObject,
    pub(crate) model: String,
}

pub(crate) fn endpoint_path(format: WireFormat) -> &'static str {
    match format {
        WireFormat::OpenaiChat => openai_chat::PATH,
        WireFormat::AnthropicMessages => anthropic_messages::PATH,
    }
}

pub(crate) fn request_id_header(format: WireFormat) -> &'static str {
    match format {
        WireFormat::OpenaiChat => openai_chat::REQUEST_ID_HEADER,
        WireFormat::AnthropicMessages => anthropic_messages::REQUEST_ID_HEADER,
    }
}

/// Builds the exact body after the schema checks.
pub(crate) fn build_body(
    config: &EndpointConfig,
    request: &CloudRequest,
) -> Result<Bytes, Failure> {
    check_request(config, request)?;
    let body = match config.wire {
        WireSettings::OpenaiChat(settings) => {
            openai_chat::request_body(&config.model, &settings, config.effort, request)
        }
        WireSettings::AnthropicMessages => {
            anthropic_messages::request_body(&config.model, config.effort, request)
        }
    };
    body.map(Bytes::from)
        .map_err(|error| Failure::RequestInvalid {
            reason: format!("the body could not be serialized: {error}"),
        })
}

fn check_request(config: &EndpointConfig, request: &CloudRequest) -> Result<(), Failure> {
    let invalid = |reason: String| Err(Failure::RequestInvalid { reason });
    if request.instructions.as_str().trim().is_empty() {
        return invalid("the instructions are empty".to_string());
    }
    if request.conversation.is_empty() {
        return invalid("the conversation has no message".to_string());
    }
    if let Some(position) = request
        .conversation
        .iter()
        .position(|turn| turn.text().trim().is_empty())
    {
        return invalid(format!("conversation message {position} is empty"));
    }
    if request.max_output_tokens == 0 || request.max_output_tokens > config.max_output_tokens {
        return invalid(format!(
            "the output token cap must be 1 to {}",
            config.max_output_tokens
        ));
    }
    Ok(())
}

/// Parses a 2xx reply strictly. It fails outright when the body
/// is not strict JSON or states no usage; otherwise the usage is returned even
/// when the reply is refused, so the budget can still check it.
pub(crate) fn parse_success(
    format: WireFormat,
    body: &[u8],
    schema: &ReplySchema,
    output_cap: u64,
) -> Result<ParsedReply, Failure> {
    json::parse_strict(body).map_err(|reason| malformed(format!("the reply body: {reason}")))?;
    let (usage, text) = match format {
        WireFormat::OpenaiChat => openai_chat::parse_reply(body, output_cap),
        WireFormat::AnthropicMessages => anthropic_messages::parse_reply(body, output_cap),
    }?;
    let result = text.and_then(|reply| reply_object(&reply.text, schema, reply.model));
    Ok(ParsedReply { usage, result })
}

fn reply_object(text: &str, schema: &ReplySchema, model: String) -> Result<AcceptedReply, Failure> {
    let value = json::parse_strict(text.as_bytes())
        .map_err(|reason| malformed(format!("the reply text: {reason}")))?;
    let JsonValue::Object(object) = value else {
        return Err(malformed("the reply text is not a JSON object".to_string()));
    };
    schema
        .check(&object)
        .map_err(|violation| Failure::ReplyBreaksSchema {
            path: violation.path,
            reason: violation.reason,
        })?;
    Ok(AcceptedReply { object, model })
}

pub(crate) fn malformed(reason: String) -> Failure {
    Failure::MalformedReply { reason }
}

/// A provider's token, stop reason or error code, kept only when it is a
/// short plain identifier so no free text from the reply enters an error.
pub(crate) fn plain_identifier(text: &str) -> Option<String> {
    let is_plain = !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'));
    is_plain.then(|| text.to_string())
}

pub(crate) fn usage_charge(
    usage: ReportedUsage,
    bytes_sent: u128,
    config: &EndpointConfig,
) -> Result<Charge, Failure> {
    Charge::for_usage(
        bytes_sent,
        usage.input_tokens,
        usage.output_tokens,
        &config.prices,
    )
    .map_err(Failure::Budget)
}

#[cfg(test)]
mod tests;
