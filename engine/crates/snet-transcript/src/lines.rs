use serde::Serialize;
use snet_cloud::Charge;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DocumentLine {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskLimitsLine {
    pub rounds: u32,
    pub seconds: u64,
    pub local_output_tokens: u64,
}

/// A task refused before anything was sent. The task ID is present only
/// when it was valid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskRefusedLine {
    pub client_time_milliseconds: i64,
    pub task_id: Option<String>,
    pub field: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskStartedLine {
    pub client_time_milliseconds: i64,
    pub task_id: String,
    pub question: String,
    pub documents: Vec<DocumentLine>,
    pub local_model: String,
    pub endpoint_name: String,
    pub host: String,
    pub cloud_model: String,
    pub policy_digest: String,
    pub cloud_prompt_sha256: String,
    pub local_prompt_sha256: String,
    pub limits: TaskLimitsLine,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuoteLine {
    pub document: String,
    pub verified: bool,
    pub citation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LocalExchangeLine {
    pub client_time_milliseconds: i64,
    pub task_id: String,
    pub round: u32,
    pub question: String,
    /// The digest of the weights that answered, as the local runtime names them.
    pub local_model_digest: Option<String>,
    pub answer: Option<serde_json::Value>,
    pub error_kind: Option<String>,
    pub quotes: Vec<QuoteLine>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CitationLine {
    pub citation: String,
    pub document: String,
    pub start: u64,
    pub end: u64,
}

/// Amounts as decimal strings, because a `u128` does not fit a JSON number
/// that every reader parses exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChargeLine {
    pub requests: String,
    pub bytes_sent: String,
    pub input_tokens: String,
    pub output_tokens: String,
}

impl From<Charge> for ChargeLine {
    fn from(charge: Charge) -> Self {
        Self {
            requests: charge.requests.to_string(),
            bytes_sent: charge.bytes_sent.to_string(),
            input_tokens: charge.input_tokens.to_string(),
            output_tokens: charge.output_tokens.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskEndedLine {
    pub client_time_milliseconds: i64,
    pub task_id: String,
    pub outcome: String,
    pub error_kind: Option<String>,
    pub answer: Option<String>,
    pub citations: Vec<CitationLine>,
    pub charge: ChargeLine,
    pub local_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FanOutLimitsLine {
    pub plan_rounds: u32,
    pub seconds: u64,
    pub local_output_tokens: u64,
    pub local_requests: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FanOutStartedLine {
    pub client_time_milliseconds: i64,
    pub task_id: String,
    pub question: String,
    pub documents: Vec<DocumentLine>,
    pub chunk_size: u64,
    pub chunk_counts: Vec<u64>,
    pub local_model: String,
    pub endpoint_name: String,
    pub host: String,
    pub cloud_model: String,
    pub policy_digest: String,
    pub cloud_prompt_sha256: String,
    pub local_prompt_sha256: String,
    pub limits: FanOutLimitsLine,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChunkRunLine {
    pub document: u32,
    pub chunk: u32,
    pub local_model_digest: Option<String>,
    pub answer: Option<serde_json::Value>,
    pub error_kind: Option<String>,
    pub quotes: Vec<QuoteLine>,
    pub outcome: String,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobRunLine {
    pub client_time_milliseconds: i64,
    pub task_id: String,
    pub round: u32,
    pub job: u32,
    pub question: String,
    pub terms: Vec<String>,
    pub chunks: Vec<ChunkRunLine>,
}

/// A question answered by the cloud model alone, with no local material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirectStartedLine {
    pub client_time_milliseconds: i64,
    pub task_id: String,
    pub question: String,
    pub earlier_exchanges: u32,
    pub endpoint_name: String,
    pub host: String,
    pub cloud_model: String,
    pub policy_digest: String,
    pub cloud_prompt_sha256: String,
    pub seconds: u64,
}

/// One step of a task's log. None of them carries the pseudonym mapping, so
/// the transcript never holds the values pseudonymisation replaced.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskRecord {
    TaskRefused(TaskRefusedLine),
    TaskStarted(TaskStartedLine),
    LocalExchange(LocalExchangeLine),
    TaskEnded(TaskEndedLine),
    FanOutStarted(FanOutStartedLine),
    JobRun(JobRunLine),
    DirectStarted(DirectStartedLine),
}
