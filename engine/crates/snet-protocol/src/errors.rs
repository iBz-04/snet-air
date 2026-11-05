/// The kinds of failure a conversation or fan-out task can end with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Input,
    Local,
    LocalLimit,
    Cloud,
    InvalidCloudReply,
    InvalidLocalReply,
    CopiedQuote,
    SpelledValue,
    ReportCap,
    Pseudonymisation,
    Record,
    Timeout,
}

impl FailureKind {
    pub fn name(self) -> &'static str {
        match self {
            FailureKind::Input => "input",
            FailureKind::Local => "local",
            FailureKind::LocalLimit => "local_limit",
            FailureKind::Cloud => "cloud",
            FailureKind::InvalidCloudReply => "invalid_cloud_reply",
            FailureKind::InvalidLocalReply => "invalid_local_reply",
            FailureKind::CopiedQuote => "copied_quote",
            FailureKind::SpelledValue => "spelled_value",
            FailureKind::ReportCap => "report_cap",
            FailureKind::Pseudonymisation => "pseudonymisation",
            FailureKind::Record => "record",
            FailureKind::Timeout => "timeout",
        }
    }
}

/// Why a task ended without an answer. `detail` names fields, limits and
/// the other crates' typed errors, never material, quotes, findings or
/// answers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the task failed ({}): {detail}", kind.name())]
pub struct ProtocolError {
    pub kind: FailureKind,
    pub detail: String,
}

impl ProtocolError {
    pub fn new(kind: FailureKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}
