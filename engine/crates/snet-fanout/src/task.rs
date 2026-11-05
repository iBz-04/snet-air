use snet_protocol::{Document, holds_bracket, name_is_valid, question_is_valid};

const MAX_DOCUMENTS: usize = 64;
const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
const PLAN_ROUNDS: std::ops::RangeInclusive<u32> = 1..=4;
const SECONDS: std::ops::RangeInclusive<u64> = 1..=86_400;
const MIN_LOCAL_OUTPUT_TOKENS: u64 = 64;
const LOCAL_REQUESTS: std::ops::RangeInclusive<u64> = 1..=4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanOutLimits {
    pub plan_rounds: u32,
    pub seconds: u64,
    pub local_output_tokens: u64,
    pub local_requests: u64,
}

/// A person's question about large local material.
#[derive(Clone)]
pub struct FanOutTask {
    pub task_id: String,
    pub question: String,
    pub documents: Vec<Document>,
    pub limits: FanOutLimits,
}

impl std::fmt::Debug for FanOutTask {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FanOutTask")
            .field("task_id", &self.task_id)
            .field("question_characters", &self.question.chars().count())
            .field("documents", &self.documents)
            .field("limits", &self.limits)
            .finish()
    }
}

/// The input checks apart from the task ID, which `snet-cloud` checks, and
/// control tokens, which `snet-local`'s check finds. Returns the first field
/// that breaks them.
pub(crate) fn invalid_field(
    task: &FanOutTask,
    endpoint_request_limit: u64,
    local_output_cap: u64,
) -> Option<String> {
    if !question_is_valid(&task.question) {
        return Some("question".to_string());
    }
    if !(1..=MAX_DOCUMENTS).contains(&task.documents.len()) {
        return Some("documents".to_string());
    }
    let mut total = 0usize;
    for (index, document) in task.documents.iter().enumerate() {
        if !name_is_valid(&document.name) {
            return Some(format!("documents[{index}].name"));
        }
        if !(1..=MAX_DOCUMENT_BYTES).contains(&document.text.len()) || holds_bracket(&document.text)
        {
            return Some(format!("documents[{index}].text"));
        }
        total += document.text.len();
    }
    if total > MAX_TEXT_BYTES {
        return Some("documents".to_string());
    }
    let limits = task.limits;
    if !PLAN_ROUNDS.contains(&limits.plan_rounds)
        || u64::from(limits.plan_rounds) + 1 > endpoint_request_limit
    {
        return Some("limits.plan_rounds".to_string());
    }
    if !SECONDS.contains(&limits.seconds) {
        return Some("limits.seconds".to_string());
    }
    if !(MIN_LOCAL_OUTPUT_TOKENS..=local_output_cap).contains(&limits.local_output_tokens) {
        return Some("limits.local_output_tokens".to_string());
    }
    if !LOCAL_REQUESTS.contains(&limits.local_requests) {
        return Some("limits.local_requests".to_string());
    }
    None
}
