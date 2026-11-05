use serde::{Deserialize, Serialize};

use super::{
    CloudRequest, ConversationRole, ReplyText, ReportedUsage, malformed, plain_identifier,
};
use crate::config::Effort;
use crate::errors::Failure;
use crate::json::describe_json_error;
use crate::truncation::{TruncatedReply, TruncationStop};

pub(crate) const PATH: &str = "/messages";
pub(crate) const REQUEST_ID_HEADER: &str = "request-id";
/// Fixed in code, so a later version goes through a spec.
pub(crate) const API_VERSION: &str = "2026-10-21";

#[derive(Serialize)]
struct RequestBody<'a> {
    model: &'a str,
    max_tokens: u64,
    system: &'a str,
    messages: Vec<Message<'a>>,
    output_config: OutputConfig<'a>,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Serialize)]
struct OutputConfig<'a> {
    format: OutputFormat<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<&'static str>,
}

#[derive(Serialize)]
struct OutputFormat<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    schema: &'a serde_json::Value,
}

/// The effort field of the body.
pub(super) fn request_body(
    model: &str,
    effort: Effort,
    request: &CloudRequest,
) -> Result<Vec<u8>, serde_json::Error> {
    let messages = request
        .conversation
        .iter()
        .map(|turn| Message {
            role: match turn.role() {
                ConversationRole::User => "user",
                ConversationRole::Assistant => "assistant",
            },
            content: turn.text(),
        })
        .collect();
    serde_json::to_vec(&RequestBody {
        model,
        max_tokens: request.max_output_tokens,
        system: request.instructions.as_str(),
        messages,
        output_config: OutputConfig {
            format: OutputFormat {
                kind: "json_schema",
                schema: request.reply_schema.checked().as_json(),
            },
            effort: effort.wire_value(),
        },
    })
}

#[derive(Deserialize)]
struct UsageOnly {
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    output_tokens_details: Option<OutputTokensDetails>,
}

#[derive(Deserialize)]
struct OutputTokensDetails {
    thinking_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct Reply {
    content: Vec<ContentBlock>,
    stop_reason: Option<String>,
    model: String,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

/// Fails when the reply states no usage. Otherwise returns the usage and
/// either the reply text with the model it names or the failure,
/// so usage is charged even for a refused reply.
pub(super) fn parse_reply(
    body: &[u8],
    output_cap: u64,
) -> Result<(ReportedUsage, Result<ReplyText, Failure>), Failure> {
    let usage = match serde_json::from_slice::<UsageOnly>(body) {
        Ok(UsageOnly { usage: Some(usage) }) => ReportedUsage {
            // Input is everything the provider read, cached or not.
            input_tokens: u128::from(usage.input_tokens)
                + u128::from(usage.cache_creation_input_tokens.unwrap_or(0))
                + u128::from(usage.cache_read_input_tokens.unwrap_or(0)),
            output_tokens: u128::from(usage.output_tokens),
            thinking_tokens: usage
                .output_tokens_details
                .and_then(|details| details.thinking_tokens)
                .map(u128::from),
        },
        Ok(UsageOnly { usage: None }) => {
            return Err(malformed("the reply states no usage".to_string()));
        }
        Err(error) => {
            let reason = format!("the usage: {}", describe_json_error(&error));
            return Err(malformed(reason));
        }
    };
    Ok((usage, reply_text(body, usage, output_cap)))
}

fn reply_text(body: &[u8], usage: ReportedUsage, output_cap: u64) -> Result<ReplyText, Failure> {
    let cut = |stop| {
        Failure::Truncated(TruncatedReply {
            stop,
            output_tokens: usage.output_tokens,
            thinking_tokens: usage.thinking_tokens,
            output_cap,
        })
    };
    let reply: Reply = serde_json::from_slice(body)
        .map_err(|error| malformed(format!("the reply: {}", describe_json_error(&error))))?;
    match reply.stop_reason.as_deref() {
        Some("end_turn") => {}
        Some("max_tokens") => return Err(cut(TruncationStop::OutputCap)),
        Some("model_context_window_exceeded") => return Err(cut(TruncationStop::ContextWindow)),
        Some("refusal") => return Err(Failure::Refused),
        Some("tool_use") => return Err(Failure::ToolRequested),
        Some("pause_turn") => return Err(Failure::Paused),
        other => {
            return Err(Failure::UnexpectedStop {
                reason: other.and_then(plain_identifier),
            });
        }
    }
    let mut text_blocks = reply
        .content
        .into_iter()
        .filter(|block| block.kind == "text");
    let (Some(block), None) = (text_blocks.next(), text_blocks.next()) else {
        return Err(malformed(
            "the reply does not have exactly one text block".to_string(),
        ));
    };
    let text = block
        .text
        .ok_or_else(|| malformed("the text block has no text".to_string()))?;
    Ok(ReplyText {
        text,
        model: reply.model,
    })
}
