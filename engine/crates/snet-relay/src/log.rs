use snet_cloud::CloudClient;
use snet_local::{LocalAnswer, LocalModel};
use snet_protocol::{FailureKind, ProtocolError, hex, now_milliseconds};
use snet_transcript::{LocalExchangeLine, RelayStartedLine, TaskRecord, Transcript};

use crate::prompts::{CLOUD_PROMPT, LOCAL_PROMPT, digest};
use crate::relay::RelayTask;

pub(crate) fn started(
    cloud: &CloudClient<Transcript>,
    local: &LocalModel,
    policy_digest: [u8; 32],
    task: &RelayTask,
) -> Result<TaskRecord, ProtocolError> {
    let config = cloud.config();
    Ok(TaskRecord::RelayStarted(RelayStartedLine {
        client_time_milliseconds: now_milliseconds()?,
        task_id: task.task_id.clone(),
        question: task.question.clone(),
        earlier_exchanges: task.earlier.len() as u32,
        local_model: local.config().model().to_string(),
        endpoint_name: config.name().to_string(),
        host: config.base_url().host().to_string(),
        cloud_model: config.model().to_string(),
        policy_digest: hex(&policy_digest),
        cloud_prompt_sha256: digest(CLOUD_PROMPT),
        local_prompt_sha256: digest(LOCAL_PROMPT),
        seconds: task.seconds,
        local_output_tokens: task.local_output_tokens,
    }))
}

/// One local step, numbered 1 for screening and 2 for the check: its reply
/// when it gave one, or the kind of failure.
pub(crate) fn exchange(
    task_id: &str,
    step: u32,
    question: &str,
    answer: Option<&LocalAnswer>,
    error_kind: Option<FailureKind>,
) -> Result<TaskRecord, ProtocolError> {
    Ok(TaskRecord::LocalExchange(LocalExchangeLine {
        client_time_milliseconds: now_milliseconds()?,
        task_id: task_id.to_string(),
        round: step,
        question: question.to_string(),
        local_model_digest: answer.map(|answer| answer.model.digest.clone()),
        answer: answer.map(|answer| {
            serde_json::Value::Object(
                answer
                    .object
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_serde_json()))
                    .collect(),
            )
        }),
        error_kind: error_kind.map(|kind| kind.name().to_string()),
        quotes: Vec::new(),
        prompt_tokens: answer.map(|answer| answer.prompt_tokens),
        completion_tokens: answer.map(|answer| answer.completion_tokens),
    }))
}
