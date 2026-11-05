use serde_json::json;
use snet_cloud::{CloudSchema, OutboundText};
use snet_local::{ConversationMessage, ConversationRole, LocalRequest, ReplySchema};
use snet_pseudonym::{OutboundJson, PseudonymError, Pseudonymiser};
use snet_wire::{JsonObject, JsonValue};

use crate::prompts::LOCAL_PROMPT;
use crate::task::Document;

pub(crate) const CLOUD_SCHEMA_NAME: &str = "conversation_step";
const LOCAL_SCHEMA_NAME: &str = "local_answer";

/// The question, the number of documents and the round limit.
pub(crate) fn task_message(
    pseudonymiser: &Pseudonymiser<'_>,
    question: OutboundText,
    documents: usize,
    rounds: u32,
) -> Result<OutboundText, PseudonymError> {
    pseudonymiser.typed(&OutboundJson::Object(vec![
        ("question", OutboundJson::Text(question)),
        ("documents", OutboundJson::Integer(documents as u32)),
        ("rounds", OutboundJson::Integer(rounds)),
    ]))
}

/// A local report, carrying citation numbers but never quotes.
pub(crate) fn report(
    pseudonymiser: &Pseudonymiser<'_>,
    round: u32,
    answer: OutboundText,
    citations: &[u32],
    unverified_quotes: u32,
    final_round: bool,
) -> Result<OutboundText, PseudonymError> {
    pseudonymiser.typed(&OutboundJson::Object(vec![
        ("round", OutboundJson::Integer(round)),
        ("answer", OutboundJson::Text(answer)),
        (
            "citations",
            OutboundJson::Array(
                citations
                    .iter()
                    .map(|number| OutboundJson::Numbered("C", *number))
                    .collect(),
            ),
        ),
        (
            "unverified_quotes",
            OutboundJson::Integer(unverified_quotes),
        ),
        ("final_round", OutboundJson::Bool(final_round)),
    ]))
}

/// `ask` before the last round, `answer` once a citation exists,
/// `not_found` always; citations as an enum of those issued.
pub(crate) fn cloud_schema(
    pseudonymiser: &Pseudonymiser<'_>,
    last_round: bool,
    issued: &[u32],
) -> Result<OutboundText, PseudonymError> {
    let mut steps = Vec::new();
    if !last_round {
        steps.push("ask");
    }
    if !issued.is_empty() {
        steps.push("answer");
    }
    steps.push("not_found");
    snet_protocol::step_schema(pseudonymiser, &steps, issued, Vec::new())
}

pub(crate) fn cloud_reply_schema(text: OutboundText) -> Result<CloudSchema, String> {
    CloudSchema::new(CLOUD_SCHEMA_NAME, text).map_err(|error| error.to_string())
}

pub(crate) fn document_id(index: usize) -> String {
    format!("D{}", index + 1)
}

/// Every document and the restored question, with the
/// reply schema of the local answer.
pub(crate) fn local_request(
    documents: &[Document],
    question: &str,
    output_tokens: u64,
) -> Result<LocalRequest, String> {
    let ids: Vec<String> = (0..documents.len()).map(document_id).collect();
    let message = json!({
        "documents": documents
            .iter()
            .enumerate()
            .map(|(index, document)| json!({"id": ids[index], "text": document.text}))
            .collect::<Vec<_>>(),
        "question": question,
    });
    let schema = ReplySchema::new(
        LOCAL_SCHEMA_NAME,
        json!({
            "type": "object",
            "properties": {
                "answer": {"type": "string"},
                "quotes": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "document": {"type": "string", "enum": ids},
                            "text": {"type": "string"}
                        },
                        "required": ["document", "text"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["answer", "quotes"],
            "additionalProperties": false
        }),
    )
    .map_err(|error| error.to_string())?;
    Ok(LocalRequest {
        instructions: LOCAL_PROMPT.to_string(),
        conversation: vec![ConversationMessage {
            role: ConversationRole::User,
            text: message.to_string(),
        }],
        reply_schema: schema,
        max_output_tokens: output_tokens,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Ask,
    Answer,
    NotFound,
}

/// A schema-checked cloud reply, read into its parts.
pub(crate) struct CloudStep {
    pub(crate) step: Step,
    pub(crate) text: String,
    pub(crate) citations: Vec<String>,
}

fn string_of<'o>(object: &'o JsonObject, key: &str) -> Option<&'o str> {
    match object.get(key) {
        Some(JsonValue::String(text)) => Some(text),
        _ => None,
    }
}

/// The schema has been checked by `snet-cloud`, so a missing part here is a
/// reply that does not match the schema this round sent.
pub(crate) fn cloud_step(object: &JsonObject) -> Option<CloudStep> {
    let step = match string_of(object, "step")? {
        "ask" => Step::Ask,
        "answer" => Step::Answer,
        "not_found" => Step::NotFound,
        _ => return None,
    };
    let text = string_of(object, "text")?.to_string();
    let Some(JsonValue::Array(items)) = object.get("citations") else {
        return None;
    };
    let mut citations = Vec::with_capacity(items.len());
    for item in items {
        let JsonValue::String(citation) = item else {
            return None;
        };
        citations.push(citation.clone());
    }
    Some(CloudStep {
        step,
        text,
        citations,
    })
}

pub(crate) struct LocalReply {
    pub(crate) answer: String,
    pub(crate) quotes: Vec<(usize, String)>,
}

pub(crate) fn local_reply(object: &JsonObject, documents: usize) -> Option<LocalReply> {
    let answer = string_of(object, "answer")?.to_string();
    let Some(JsonValue::Array(items)) = object.get("quotes") else {
        return None;
    };
    let mut quotes = Vec::with_capacity(items.len());
    for item in items {
        let JsonValue::Object(quote) = item else {
            return None;
        };
        let document = (0..documents)
            .find(|index| string_of(quote, "document") == Some(document_id(*index).as_str()))?;
        quotes.push((document, string_of(quote, "text")?.to_string()));
    }
    Some(LocalReply { answer, quotes })
}
