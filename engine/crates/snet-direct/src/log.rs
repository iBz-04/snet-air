use snet_cloud::CloudClient;
use snet_protocol::{ProtocolError, hex, now_milliseconds};
use snet_transcript::{DirectStartedLine, TaskRecord, Transcript};

use crate::direct::DirectTask;
use crate::prompts::{CLOUD_PROMPT, digest};

pub(crate) fn started(
    cloud: &CloudClient<Transcript>,
    policy_digest: [u8; 32],
    task: &DirectTask,
) -> Result<TaskRecord, ProtocolError> {
    let config = cloud.config();
    Ok(TaskRecord::DirectStarted(DirectStartedLine {
        client_time_milliseconds: now_milliseconds()?,
        task_id: task.task_id.clone(),
        question: task.question.clone(),
        earlier_exchanges: task.earlier.len() as u32,
        endpoint_name: config.name().to_string(),
        host: config.base_url().host().to_string(),
        cloud_model: config.model().to_string(),
        policy_digest: hex(&policy_digest),
        cloud_prompt_sha256: digest(CLOUD_PROMPT),
        seconds: task.seconds,
    }))
}
