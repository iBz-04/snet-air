const MAX_DOCUMENTS: usize = 16;
const MAX_DOCUMENT_BYTES: usize = 64 * 1024;
const ROUNDS: std::ops::RangeInclusive<u32> = 2..=8;
const SECONDS: std::ops::RangeInclusive<u64> = 1..=3_600;
const MIN_LOCAL_OUTPUT_TOKENS: u64 = 64;

pub use snet_protocol::Document;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversationLimits {
    pub rounds: u32,
    pub seconds: u64,
    pub local_output_tokens: u64,
}

/// A person's question about local documents.
#[derive(Clone)]
pub struct ConversationTask {
    pub task_id: String,
    pub question: String,
    pub documents: Vec<Document>,
    pub limits: ConversationLimits,
}

impl std::fmt::Debug for ConversationTask {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConversationTask")
            .field("task_id", &self.task_id)
            .field("question_characters", &self.question.chars().count())
            .field("documents", &self.documents)
            .field("limits", &self.limits)
            .finish()
    }
}

use snet_protocol::{holds_bracket, name_is_valid, question_is_valid};

/// The input checks apart from the task ID, which `snet-cloud` checks when
/// the budget is made. Returns the first field that breaks them.
pub(crate) fn invalid_field(
    task: &ConversationTask,
    endpoint_request_limit: u64,
    local_output_cap: u64,
) -> Option<String> {
    if !question_is_valid(&task.question) {
        return Some("question".to_string());
    }
    if !(1..=MAX_DOCUMENTS).contains(&task.documents.len()) {
        return Some("documents".to_string());
    }
    for (index, document) in task.documents.iter().enumerate() {
        if !name_is_valid(&document.name) {
            return Some(format!("documents[{index}].name"));
        }
        if !(1..=MAX_DOCUMENT_BYTES).contains(&document.text.len()) || holds_bracket(&document.text)
        {
            return Some(format!("documents[{index}].text"));
        }
    }
    let limits = task.limits;
    if !ROUNDS.contains(&limits.rounds) || u64::from(limits.rounds) > endpoint_request_limit {
        return Some("limits.rounds".to_string());
    }
    if !SECONDS.contains(&limits.seconds) {
        return Some("limits.seconds".to_string());
    }
    if !(MIN_LOCAL_OUTPUT_TOKENS..=local_output_cap).contains(&limits.local_output_tokens) {
        return Some("limits.local_output_tokens".to_string());
    }
    None
}
