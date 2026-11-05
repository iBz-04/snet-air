use sha2::{Digest, Sha256};
use snet_cloud::CloudClient;
use snet_local::LocalModel;
use snet_protocol::{ProtocolError, hex, now_milliseconds};
use snet_transcript::{
    ChunkRunLine, DocumentLine, FanOutLimitsLine, FanOutStartedLine, JobRunLine, TaskRecord,
    Transcript,
};

use crate::prompts::{CLOUD_PROMPT, LOCAL_PROMPT, digest};
use crate::task::FanOutTask;

pub(crate) struct Start<'s> {
    pub(crate) cloud: &'s CloudClient<Transcript>,
    pub(crate) local: &'s LocalModel,
    pub(crate) policy_digest: [u8; 32],
    pub(crate) chunk_size: usize,
    pub(crate) chunk_counts: Vec<u64>,
}

pub(crate) fn started(start: Start<'_>, task: &FanOutTask) -> Result<TaskRecord, ProtocolError> {
    let config = start.cloud.config();
    Ok(TaskRecord::FanOutStarted(FanOutStartedLine {
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
        chunk_size: start.chunk_size as u64,
        chunk_counts: start.chunk_counts,
        local_model: start.local.config().model().to_string(),
        endpoint_name: config.name().to_string(),
        host: config.base_url().host().to_string(),
        cloud_model: config.model().to_string(),
        policy_digest: hex(&start.policy_digest),
        cloud_prompt_sha256: digest(CLOUD_PROMPT),
        local_prompt_sha256: digest(LOCAL_PROMPT),
        limits: FanOutLimitsLine {
            plan_rounds: task.limits.plan_rounds,
            seconds: task.limits.seconds,
            local_output_tokens: task.limits.local_output_tokens,
            local_requests: task.limits.local_requests,
        },
    }))
}

/// One job's runs in a round.
pub(crate) struct JobRuns {
    pub(crate) round: u32,
    pub(crate) job: u32,
    pub(crate) question: String,
    pub(crate) terms: Vec<String>,
    pub(crate) chunks: Vec<ChunkRunLine>,
}

pub(crate) fn job_record(task_id: &str, runs: JobRuns) -> Result<TaskRecord, ProtocolError> {
    Ok(TaskRecord::JobRun(JobRunLine {
        client_time_milliseconds: now_milliseconds()?,
        task_id: task_id.to_string(),
        round: runs.round,
        job: runs.job,
        question: runs.question,
        terms: runs.terms,
        chunks: runs.chunks,
    }))
}
