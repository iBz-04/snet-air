use serde_json::json;
use snet_cloud::{CloudSchema, OutboundText};
use snet_local::{ConversationMessage, ConversationRole, LocalRequest, ReplySchema};
use snet_pseudonym::{OutboundJson, PseudonymError, Pseudonymiser};
use snet_wire::{JsonObject, JsonValue};

use crate::prompts::LOCAL_PROMPT;

pub(crate) const CLOUD_SCHEMA_NAME: &str = "fan_out_step";
const LOCAL_SCHEMA_NAME: &str = "chunk_finding";
/// The discard reasons, in the order they are tried.
pub(crate) const DISCARD_REASONS: [&str; 6] = [
    "not_relevant",
    "too_long",
    "unverified",
    "copied_quote",
    "bracket",
    "spelled_value",
];

pub(crate) fn task_message(
    pseudonymiser: &Pseudonymiser<'_>,
    question: OutboundText,
    chunk_counts: &[u32],
    plan_rounds: u32,
    local_requests_left: u32,
) -> Result<OutboundText, PseudonymError> {
    pseudonymiser.typed(&OutboundJson::Object(vec![
        ("question", OutboundJson::Text(question)),
        (
            "chunks",
            OutboundJson::Array(
                chunk_counts
                    .iter()
                    .map(|count| OutboundJson::Integer(*count))
                    .collect(),
            ),
        ),
        ("plan_rounds", OutboundJson::Integer(plan_rounds)),
        (
            "local_requests_left",
            OutboundJson::Integer(local_requests_left),
        ),
    ]))
}

/// One result sent to the cloud.
pub(crate) struct SentResult {
    pub(crate) document: u32,
    pub(crate) chunk: u32,
    pub(crate) finding: OutboundText,
    pub(crate) citation: u32,
}

/// One job's part of a report.
pub(crate) struct JobReport {
    pub(crate) chunks: u32,
    pub(crate) failed: u32,
    pub(crate) discarded: [u32; 6],
    pub(crate) held_back: u32,
    pub(crate) results: Vec<SentResult>,
}

pub(crate) fn report(
    pseudonymiser: &Pseudonymiser<'_>,
    round: u32,
    local_requests_left: u32,
    jobs: Vec<JobReport>,
) -> Result<OutboundText, PseudonymError> {
    let jobs = jobs
        .into_iter()
        .enumerate()
        .map(|(position, job)| {
            OutboundJson::Object(vec![
                ("job", OutboundJson::Integer(position as u32 + 1)),
                ("chunks", OutboundJson::Integer(job.chunks)),
                ("failed", OutboundJson::Integer(job.failed)),
                (
                    "discarded",
                    OutboundJson::Object(
                        DISCARD_REASONS
                            .iter()
                            .zip(job.discarded)
                            .map(|(reason, count)| (*reason, OutboundJson::Integer(count)))
                            .collect(),
                    ),
                ),
                ("held_back", OutboundJson::Integer(job.held_back)),
                (
                    "results",
                    OutboundJson::Array(
                        job.results
                            .into_iter()
                            .map(|result| {
                                OutboundJson::Object(vec![
                                    (
                                        "chunk",
                                        OutboundJson::Array(vec![
                                            OutboundJson::Integer(result.document),
                                            OutboundJson::Integer(result.chunk),
                                        ]),
                                    ),
                                    ("finding", OutboundJson::Text(result.finding)),
                                    ("citation", OutboundJson::Numbered("C", result.citation)),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ])
        })
        .collect();
    pseudonymiser.typed(&OutboundJson::Object(vec![
        ("round", OutboundJson::Integer(round)),
        (
            "local_requests_left",
            OutboundJson::Integer(local_requests_left),
        ),
        ("jobs", OutboundJson::Array(jobs)),
    ]))
}

pub(crate) fn cloud_schema(
    pseudonymiser: &Pseudonymiser<'_>,
    last_request: bool,
    issued: &[u32],
) -> Result<OutboundText, PseudonymError> {
    let mut steps = Vec::new();
    if !last_request {
        steps.push("plan");
    }
    if !issued.is_empty() {
        steps.push("answer");
    }
    steps.push("not_found");
    let string = || OutboundJson::Object(vec![("type", OutboundJson::Static("string"))]);
    let jobs = OutboundJson::Object(vec![
        ("type", OutboundJson::Static("array")),
        (
            "items",
            OutboundJson::Object(vec![
                ("type", OutboundJson::Static("object")),
                (
                    "properties",
                    OutboundJson::Object(vec![
                        ("question", string()),
                        (
                            "terms",
                            OutboundJson::Object(vec![
                                ("type", OutboundJson::Static("array")),
                                ("items", string()),
                            ]),
                        ),
                    ]),
                ),
                (
                    "required",
                    OutboundJson::Array(vec![
                        OutboundJson::Static("question"),
                        OutboundJson::Static("terms"),
                    ]),
                ),
                ("additionalProperties", OutboundJson::Bool(false)),
            ]),
        ),
    ]);
    snet_protocol::step_schema(pseudonymiser, &steps, issued, vec![("jobs", jobs)])
}

pub(crate) fn cloud_reply_schema(text: OutboundText) -> Result<CloudSchema, String> {
    CloudSchema::new(CLOUD_SCHEMA_NAME, text).map_err(|error| error.to_string())
}

/// One chunk and one restored job question.
pub(crate) fn local_request(
    chunk: &str,
    question: &str,
    output_tokens: u64,
) -> Result<LocalRequest, String> {
    let schema = ReplySchema::new(
        LOCAL_SCHEMA_NAME,
        json!({
            "type": "object",
            "properties": {
                "relevant": {"type": "boolean"},
                "finding": {"type": "string"},
                "quotes": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["relevant", "finding", "quotes"],
            "additionalProperties": false
        }),
    )
    .map_err(|error| error.to_string())?;
    Ok(LocalRequest {
        instructions: LOCAL_PROMPT.to_string(),
        conversation: vec![ConversationMessage {
            role: ConversationRole::User,
            text: json!({"chunk": chunk, "question": question}).to_string(),
        }],
        reply_schema: schema,
        max_output_tokens: output_tokens,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Plan,
    Answer,
    NotFound,
}

pub(crate) struct Job {
    pub(crate) question: String,
    pub(crate) terms: Vec<String>,
}

pub(crate) struct CloudStep {
    pub(crate) step: Step,
    pub(crate) text: String,
    pub(crate) jobs: Vec<Job>,
    pub(crate) citations: Vec<String>,
}

fn string_of<'o>(object: &'o JsonObject, key: &str) -> Option<&'o str> {
    match object.get(key) {
        Some(JsonValue::String(text)) => Some(text),
        _ => None,
    }
}

fn strings_of(value: Option<&JsonValue>) -> Option<Vec<String>> {
    let Some(JsonValue::Array(items)) = value else {
        return None;
    };
    items
        .iter()
        .map(|item| match item {
            JsonValue::String(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// A schema-checked cloud reply read into its parts.
pub(crate) fn cloud_step(object: &JsonObject) -> Option<CloudStep> {
    let step = match string_of(object, "step")? {
        "plan" => Step::Plan,
        "answer" => Step::Answer,
        "not_found" => Step::NotFound,
        _ => return None,
    };
    let Some(JsonValue::Array(items)) = object.get("jobs") else {
        return None;
    };
    let mut jobs = Vec::with_capacity(items.len());
    for item in items {
        let JsonValue::Object(job) = item else {
            return None;
        };
        jobs.push(Job {
            question: string_of(job, "question")?.to_string(),
            terms: strings_of(job.get("terms"))?,
        });
    }
    Some(CloudStep {
        step,
        text: string_of(object, "text")?.to_string(),
        jobs,
        citations: strings_of(object.get("citations"))?,
    })
}

/// A schema-checked local answer read into its parts.
pub(crate) struct LocalResult {
    pub(crate) relevant: bool,
    pub(crate) finding: String,
    pub(crate) quotes: Vec<String>,
}

pub(crate) fn local_result(object: &JsonObject) -> Option<LocalResult> {
    let Some(JsonValue::Bool(relevant)) = object.get("relevant") else {
        return None;
    };
    Some(LocalResult {
        relevant: *relevant,
        finding: string_of(object, "finding")?.to_string(),
        quotes: strings_of(object.get("quotes"))?,
    })
}
