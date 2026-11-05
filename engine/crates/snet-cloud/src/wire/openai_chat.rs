use serde::{Deserialize, Serialize};

use super::{
    CloudRequest, ConversationRole, ReplyText, ReportedUsage, malformed, plain_identifier,
};
use crate::config::{Effort, InstructionRole, OpenaiChatSettings};
use crate::errors::Failure;
use crate::json::describe_json_error;
use crate::truncation::{TruncatedReply, TruncationStop};

pub(crate) const PATH: &str = "/chat/completions";
pub(crate) const REQUEST_ID_HEADER: &str = "x-request-id";

#[derive(Serialize)]
struct RequestBody<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
    max_completion_tokens: u64,
    n: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'static str>,
    response_format: ResponseFormat<'a>,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Serialize)]
struct ResponseFormat<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    json_schema: JsonSchemaFormat<'a>,
}

#[derive(Serialize)]
struct JsonSchemaFormat<'a> {
    name: &'a str,
    schema: &'a serde_json::Value,
    strict: bool,
}

/// The effort field of the body.
pub(super) fn request_body(
    model: &str,
    settings: &OpenaiChatSettings,
    effort: Effort,
    request: &CloudRequest,
) -> Result<Vec<u8>, serde_json::Error> {
    let instruction_role = match settings.instruction_role {
        InstructionRole::System => "system",
        InstructionRole::Developer => "developer",
    };
    let mut messages = Vec::with_capacity(request.conversation.len() + 1);
    messages.push(Message {
        role: instruction_role,
        content: request.instructions.as_str(),
    });
    for turn in &request.conversation {
        messages.push(Message {
            role: match turn.role() {
                ConversationRole::User => "user",
                ConversationRole::Assistant => "assistant",
            },
            content: turn.text(),
        });
    }
    serde_json::to_vec(&RequestBody {
        model,
        messages,
        max_completion_tokens: request.max_output_tokens,
        n: 1,
        store: settings.send_store_false.then_some(false),
        reasoning_effort: effort.wire_value(),
        response_format: ResponseFormat {
            kind: "json_schema",
            json_schema: JsonSchemaFormat {
                name: request.reply_schema.checked().name(),
                schema: request.reply_schema.checked().as_json(),
                strict: true,
            },
        },
    })
}

#[derive(Deserialize)]
struct UsageOnly {
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
    completion_tokens_details: Option<CompletionTokensDetails>,
}

#[derive(Deserialize)]
struct CompletionTokensDetails {
    reasoning_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct Reply {
    choices: Vec<Choice>,
    model: String,
}

#[derive(Deserialize)]
struct Choice {
    message: ReplyMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ReplyMessage {
    content: Option<String>,
    refusal: Option<String>,
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
            input_tokens: u128::from(usage.prompt_tokens),
            output_tokens: u128::from(usage.completion_tokens),
            thinking_tokens: usage
                .completion_tokens_details
                .and_then(|details| details.reasoning_tokens)
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
    let reply: Reply = serde_json::from_slice(body)
        .map_err(|error| malformed(format!("the reply: {}", describe_json_error(&error))))?;
    let choice_count = reply.choices.len();
    let Ok([choice]) = <[Choice; 1]>::try_from(reply.choices) else {
        return Err(malformed(format!(
            "the reply has {choice_count} choices instead of one"
        )));
    };
    if choice.message.refusal.is_some() {
        return Err(Failure::Refused);
    }
    match choice.finish_reason.as_deref() {
        Some("stop") => {}
        Some("length") => {
            return Err(Failure::Truncated(TruncatedReply {
                stop: TruncationStop::Length,
                output_tokens: usage.output_tokens,
                thinking_tokens: usage.thinking_tokens,
                output_cap,
            }));
        }
        Some("content_filter") => return Err(Failure::Filtered),
        Some("tool_calls" | "function_call") => return Err(Failure::ToolRequested),
        other => {
            return Err(Failure::UnexpectedStop {
                reason: other.and_then(plain_identifier),
            });
        }
    }
    let text = choice
        .message
        .content
        .ok_or_else(|| malformed("the choice has no text".to_string()))?;
    Ok(ReplyText {
        text,
        model: reply.model,
    })
}
