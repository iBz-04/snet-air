use std::time::{SystemTime, UNIX_EPOCH};

use snet_cloud::Charge;
use snet_transcript::{
    ChargeLine, CitationLine, TaskEndedLine, TaskRecord, TaskRefusedLine, Transcript,
};

use crate::citations::CitedPassage;
use crate::errors::{FailureKind, ProtocolError};

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn now_milliseconds() -> Result<i64, ProtocolError> {
    let clock = |detail| ProtocolError::new(FailureKind::Record, detail);
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| clock("the clock is before 1970"))?;
    i64::try_from(elapsed.as_millis()).map_err(|_| clock("the clock is too far in the future"))
}

/// Appends a task step to the transcript; any failure is kind `record`.
pub fn append(transcript: &Transcript, record: TaskRecord) -> Result<u64, ProtocolError> {
    transcript
        .record_task(&record)
        .map_err(|error| ProtocolError::new(FailureKind::Record, error.to_string()))
}

/// A task refused before anything was sent.
pub fn refused(task_id: Option<&str>, field: &str) -> Result<TaskRecord, ProtocolError> {
    Ok(TaskRecord::TaskRefused(TaskRefusedLine {
        client_time_milliseconds: now_milliseconds()?,
        task_id: task_id.map(str::to_string),
        field: field.to_string(),
    }))
}

/// What a task-end line holds.
pub struct Ending<'e> {
    pub outcome: &'static str,
    pub error_kind: Option<FailureKind>,
    pub answer: Option<&'e str>,
    pub citations: &'e [CitedPassage],
    pub charge: Charge,
    pub local_tokens: u64,
}

pub fn ended(task_id: &str, ending: Ending<'_>) -> Result<TaskRecord, ProtocolError> {
    Ok(TaskRecord::TaskEnded(TaskEndedLine {
        client_time_milliseconds: now_milliseconds()?,
        task_id: task_id.to_string(),
        outcome: ending.outcome.to_string(),
        error_kind: ending.error_kind.map(|kind| kind.name().to_string()),
        answer: ending.answer.map(str::to_string),
        citations: ending
            .citations
            .iter()
            .map(|passage| CitationLine {
                citation: passage.citation.clone(),
                document: passage.document.clone(),
                start: passage.range.start as u64,
                end: passage.range.end as u64,
            })
            .collect(),
        charge: ChargeLine::from(ending.charge),
        local_tokens: ending.local_tokens,
    }))
}
