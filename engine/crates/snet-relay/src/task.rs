use snet_local::LocalModel;
use snet_protocol::{FailureKind, ProtocolError, holds_bracket, question_is_valid};

use crate::screen;

/// How many earlier exchanges of a session go to the local model with a
/// new question.
pub const MAX_EARLIER_EXCHANGES: usize = 8;
pub(crate) const MAX_ANSWER_CHARACTERS: usize = 32_000;
const SECONDS: std::ops::RangeInclusive<u64> = 1..=3_600;
const MIN_LOCAL_OUTPUT_TOKENS: u64 = 64;

/// A question asked earlier in the session and the answer it got, both as
/// the person saw them. They stay on the machine: the local model folds
/// what is needed into the next cloud question.
#[derive(Clone)]
pub struct EarlierExchange {
    pub question: String,
    pub answer: String,
}

/// A person's question without files.
#[derive(Clone)]
pub struct RelayTask {
    pub task_id: String,
    pub question: String,
    pub earlier: Vec<EarlierExchange>,
    pub seconds: u64,
    pub local_output_tokens: u64,
}

impl std::fmt::Debug for RelayTask {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RelayTask")
            .field("task_id", &self.task_id)
            .field("question_characters", &self.question.chars().count())
            .field("earlier", &self.earlier.len())
            .field("seconds", &self.seconds)
            .finish()
    }
}

/// Which model answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// The local model answered; nothing left the machine.
    Local,
    /// The cloud model answered this question, exactly as it read it,
    /// pseudonyms included, so the person can see what left the machine.
    Cloud { sent_question: String },
}

/// Runs one screening pass over a greeting and drops its answer, so the
/// model is loaded and its runtime holds the instructions in its prompt
/// cache before the person's first question.
pub async fn prime(local: &LocalModel, local_output_tokens: u64) -> Result<(), ProtocolError> {
    let task = RelayTask {
        task_id: "prime".to_string(),
        question: "hi".to_string(),
        earlier: Vec::new(),
        seconds: 60,
        local_output_tokens,
    };
    let request = screen::request(&task)?;
    local
        .ask(&request)
        .await
        .map(|_answer| ())
        .map_err(|error| ProtocolError::new(FailureKind::Local, error.to_string()))
}

/// The first field that breaks the input checks.
pub(crate) fn invalid_field(task: &RelayTask, local_output_cap: u64) -> Option<String> {
    if !question_is_valid(&task.question) {
        return Some("question".to_string());
    }
    if task.earlier.len() > MAX_EARLIER_EXCHANGES {
        return Some("earlier".to_string());
    }
    for (index, exchange) in task.earlier.iter().enumerate() {
        if !question_is_valid(&exchange.question) {
            return Some(format!("earlier[{index}].question"));
        }
        if exchange.answer.trim().is_empty()
            || exchange.answer.chars().count() > MAX_ANSWER_CHARACTERS
            || holds_bracket(&exchange.answer)
        {
            return Some(format!("earlier[{index}].answer"));
        }
    }
    if !SECONDS.contains(&task.seconds) {
        return Some("seconds".to_string());
    }
    if !(MIN_LOCAL_OUTPUT_TOKENS..=local_output_cap).contains(&task.local_output_tokens) {
        return Some("local_output_tokens".to_string());
    }
    None
}
