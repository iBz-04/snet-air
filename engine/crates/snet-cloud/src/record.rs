use std::fmt;
use std::future::Future;
use std::time::SystemTime;

use bytes::Bytes;

use crate::budget::Charge;
use crate::config::WireFormat;

/// What the client hands the recorder before it opens any connection
///. `body` is exactly the bytes that are sent, and
/// `headers` leaves out the key header.
#[derive(Debug, Clone)]
pub struct OutboundRecord {
    pub time: SystemTime,
    pub task_id: String,
    pub endpoint_name: String,
    pub host: String,
    pub port: u16,
    pub model: String,
    pub wire_format: WireFormat,
    pub configuration_digest: [u8; 32],
    pub method: &'static str,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
    pub body_size: u64,
    pub body_sha256: [u8; 32],
    /// The pseudonymisation report of each text, in request order.
    pub pseudonymisation: Vec<snet_pseudonym::Report>,
}

/// The identifier the recorder gave an outbound record, which links its
/// outcome to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutboundRecordId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BytesWritten {
    None,
    Some,
    All,
}

/// How a recorded exchange ended. `usage_charged` is what the task
/// budget now holds for this request: the reported usage of an accepted
/// reply, or the reservation kept in full.
#[derive(Debug, Clone)]
pub struct OutcomeRecord {
    pub outbound: OutboundRecordId,
    pub time: SystemTime,
    pub bytes_written: BytesWritten,
    pub http_status: Option<u16>,
    pub provider_request_id: Option<String>,
    pub reply_body: Bytes,
    pub reply_size: u64,
    pub reply_sha256: [u8; 32],
    pub usage_charged: Charge,
    pub error_kind: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStage {
    Outbound,
    Outcome,
}

impl fmt::Display for RecordStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Outbound => "outbound",
            Self::Outcome => "outcome",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct RecorderError {
    message: String,
}

impl RecorderError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Where every outbound message is recorded before it is sent.
///
/// Contract for implementers:
///
/// - Return success only once the record is durable. The client opens no
///   connection until `record_outbound` has returned success, and returns no
///   reply until `record_outcome` has.
/// - An outbound record that never receives an outcome means the request was
///   possibly sent, for example because the process ended mid-exchange.
/// - Each call must return within 30 seconds; the client treats a slower call
///   as a failure.
pub trait OutboundRecorder: Send + Sync + 'static {
    fn record_outbound(
        &self,
        record: OutboundRecord,
    ) -> impl Future<Output = Result<OutboundRecordId, RecorderError>> + Send;

    fn record_outcome(
        &self,
        outcome: OutcomeRecord,
    ) -> impl Future<Output = Result<(), RecorderError>> + Send;
}
