use sha2::{Digest, Sha256};
use snet_cloud::CloudClient;
use snet_local::LocalModel;
use snet_transcript::{
    DocumentLine, LocalExchangeLine, QuoteLine, TaskLimitsLine, TaskRecord, TaskStartedLine,
    Transcript,
};
use snet_wire::JsonObject;

use crate::errors::{ConversationError, FailureKind};
use crate::prompts::{CLOUD_PROMPT, LOCAL_PROMPT, digest};
use crate::task::ConversationTask;
pub(crate) use snet_protocol::{Ending, append, ended, refused};
use snet_protocol::{hex, now_milliseconds};

pub(crate) fn started(
    cloud: &CloudClient<Transcript>,
    local: &LocalModel,
    policy_digest: [u8; 32],
    task: &ConversationTask,
) -> Result<TaskRecord, ConversationError> {
    let config = cloud.config();
    Ok(TaskRecord::TaskStarted(TaskStartedLine {
        client_time_milliseconds: now_milliseconds()?,
        task_id: task.task_id.clone(),
        question: task.question.clone(),
        documents: task
            .documents
            .iter()
            .map(|document| DocumentLine {
                name: document.name.clone(),
                size: document.text.len() as u64,
                sha256: hex(&Sha256::digest(document.text.as_bytes())),
            })
            .collect(),
        local_model: local.config().model().to_string(),
        endpoint_name: config.name().to_string(),
        host: config.base_url().host().to_string(),
        cloud_model: config.model().to_string(),
        policy_digest: hex(&policy_digest),
        cloud_prompt_sha256: digest(CLOUD_PROMPT),
        local_prompt_sha256: digest(LOCAL_PROMPT),
        limits: TaskLimitsLine {
            rounds: task.limits.rounds,
            seconds: task.limits.seconds,
            local_output_tokens: task.limits.local_output_tokens,
        },
    }))
}

/// What one local exchange produced, for its transcript line.
pub(crate) struct Exchange {
    pub(crate) round: u32,
    pub(crate) question: String,
    pub(crate) digest: Option<String>,
    pub(crate) answer: Option<JsonObject>,
    pub(crate) error_kind: Option<FailureKind>,
    pub(crate) quotes: Vec<(String, Option<u32>)>,
    pub(crate) tokens: Option<(u64, u64)>,
}

pub(crate) fn exchange(task_id: &str, exchange: Exchange) -> Result<TaskRecord, ConversationError> {
    Ok(TaskRecord::LocalExchange(LocalExchangeLine {
        client_time_milliseconds: now_milliseconds()?,
        task_id: task_id.to_string(),
        round: exchange.round,
        question: exchange.question,
        local_model_digest: exchange.digest,
        answer: exchange.answer.map(|object| {
            serde_json::Value::Object(
                object
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_serde_json()))
                    .collect(),
            )
        }),
        error_kind: exchange.error_kind.map(|kind| kind.name().to_string()),
        quotes: exchange
            .quotes
            .into_iter()
            .map(|(document, citation)| QuoteLine {
                document,
                verified: citation.is_some(),
                citation: citation.map(|number| format!("C{number}")),
            })
            .collect(),
        prompt_tokens: exchange.tokens.map(|(prompt, _)| prompt),
        completion_tokens: exchange.tokens.map(|(_, completion)| completion),
    }))
}
