use crate::budget::BudgetError;
use crate::record::{RecordStage, RecorderError};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("the endpoint configuration does not parse: {message}")]
    Parse { message: String },
    #[error("endpoint configuration field `{field}` is invalid: {reason}")]
    Invalid { field: &'static str, reason: String },
}

impl ConfigError {
    pub(crate) fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            reason: reason.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("cloud endpoint `{endpoint}`: {failure}")]
pub struct CloudError {
    pub endpoint: String,
    pub failure: Failure,
}

/// Every message names only the endpoint, fields, statuses, the provider's
/// error type and code and the request ID. Request text, reply text and the
/// API key never appear in one.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    #[error("the API key is unusable: {reason}")]
    KeyInvalid { reason: &'static str },
    #[error("the TLS configuration could not be built: {cause}")]
    TlsConfiguration { cause: String },
    #[error("the cloud switch is off")]
    SwitchOff,
    #[error("the request is invalid: {reason}")]
    RequestInvalid { reason: String },
    #[error("the request body is {size} bytes, above the cap of {cap}")]
    RequestTooLarge { size: u64, cap: u64 },
    #[error("the task budget refused the request: {0}")]
    Budget(BudgetError),
    #[error("the recorder failed on the {stage} record: {source}")]
    RecorderFailed {
        stage: RecordStage,
        source: RecorderError,
    },
    #[error("the recorder did not return within 30 seconds on the {stage} record")]
    RecorderTimedOut { stage: RecordStage },
    #[error("resolving `{host}` failed: {cause}")]
    Resolve { host: String, cause: String },
    #[error("`{host}` resolved to no address that the configuration permits")]
    NoPermittedAddress { host: String },
    #[error("connecting failed: {cause}")]
    Connect { cause: String },
    #[error("resolving, connecting and the TLS handshake took longer than the connect timeout")]
    ConnectTimeout,
    #[error("the exchange took longer than the total timeout")]
    TotalTimeout,
    #[error("the TLS handshake failed: {cause}")]
    Tls { cause: String },
    #[error("the cloud switch turned off during the exchange")]
    SwitchAborted,
    #[error("the HTTP exchange failed: {cause}")]
    Http { cause: String },
    #[error("the reply head is malformed or exceeds 64 headers or 65 536 bytes: {cause}")]
    ReplyHeadRefused { cause: String },
    #[error("the reply body exceeds the cap of {cap} bytes")]
    ReplyTooLarge { cap: u64 },
    #[error("the endpoint answered with redirect status {status}, which is never followed")]
    Redirect { status: u16 },
    #[error("the reply has a content encoding other than identity")]
    UnexpectedEncoding,
    #[error("the reply's content type is not application/json")]
    UnexpectedContentType,
    #[error(
        "the provider answered status {status} (error type {}, code {}, request ID {}, {})",
        display_optional(.error_type),
        display_optional(.error_code),
        display_optional(.request_id),
        if *.retryable { "retryable" } else { "not retryable" }
    )]
    Provider {
        status: u16,
        error_type: Option<String>,
        error_code: Option<String>,
        request_id: Option<String>,
        retryable: bool,
    },
    #[error("{0}")]
    Truncated(crate::truncation::TruncatedReply),
    #[error("the model refused to answer")]
    Refused,
    #[error("the provider's content filter stopped the reply")]
    Filtered,
    #[error("the model asked for a tool call, which is never run")]
    ToolRequested,
    #[error("the model paused its turn")]
    Paused,
    #[error("the reply ended with an unexpected stop reason {}", display_optional(.reason))]
    UnexpectedStop { reason: Option<String> },
    #[error("the reply is malformed: {reason}")]
    MalformedReply { reason: String },
    #[error("the reply object breaks the reply schema at {path}: {reason}")]
    ReplyBreaksSchema { path: String, reason: String },
    #[error(
        "the provider reported more usage than the request's worst case; the task budget is closed"
    )]
    UsageAboveWorstCase,
    #[error(
        "the caller stopped waiting before the reply was settled; the reservation stays charged"
    )]
    Abandoned,
    #[error("the exchange task ended without a result: {cause}")]
    ExchangeTaskFailed { cause: String },
    #[error("a text or reply in the request belongs to another endpoint")]
    DestinationMismatch,
    #[error("the request's texts were pseudonymised under different policies")]
    PolicyMismatch,
}

impl Failure {
    /// The stable name of the failure, written into the outcome record.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::KeyInvalid { .. } => "key_invalid",
            Self::TlsConfiguration { .. } => "tls_configuration",
            Self::SwitchOff => "switch_off",
            Self::RequestInvalid { .. } => "request_invalid",
            Self::RequestTooLarge { .. } => "request_too_large",
            Self::Budget(_) => "budget",
            Self::RecorderFailed { .. } => "recorder_failed",
            Self::RecorderTimedOut { .. } => "recorder_timed_out",
            Self::Resolve { .. } => "resolve",
            Self::NoPermittedAddress { .. } => "no_permitted_address",
            Self::Connect { .. } => "connect",
            Self::ConnectTimeout => "connect_timeout",
            Self::TotalTimeout => "total_timeout",
            Self::Tls { .. } => "tls",
            Self::SwitchAborted => "switch_aborted",
            Self::Http { .. } => "http",
            Self::ReplyHeadRefused { .. } => "reply_head_refused",
            Self::ReplyTooLarge { .. } => "reply_too_large",
            Self::Redirect { .. } => "redirect",
            Self::UnexpectedEncoding => "unexpected_encoding",
            Self::UnexpectedContentType => "unexpected_content_type",
            Self::Provider { .. } => "provider",
            Self::Truncated(_) => "truncated",
            Self::Refused => "refused",
            Self::Filtered => "filtered",
            Self::ToolRequested => "tool_requested",
            Self::Paused => "paused",
            Self::UnexpectedStop { .. } => "unexpected_stop",
            Self::MalformedReply { .. } => "malformed_reply",
            Self::ReplyBreaksSchema { .. } => "reply_breaks_schema",
            Self::UsageAboveWorstCase => "usage_above_worst_case",
            Self::Abandoned => "abandoned",
            Self::ExchangeTaskFailed { .. } => "exchange_task_failed",
            Self::DestinationMismatch => "destination_mismatch",
            Self::PolicyMismatch => "policy_mismatch",
        }
    }
}

fn display_optional(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("not stated")
}
