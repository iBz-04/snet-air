use std::collections::BTreeMap;
use std::future::Future;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use snet_cloud::{
    BytesWritten, OutboundRecord, OutboundRecordId, OutboundRecorder, OutcomeRecord, RecorderError,
    WireFormat,
};

use crate::lines::ChargeLine;
use crate::writer::Transcript;

#[derive(Serialize)]
struct PseudonymisationLine {
    replaced: BTreeMap<String, u64>,
    passed_through: BTreeMap<usize, u64>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename = "outbound")]
struct OutboundLine<'r> {
    line_time_milliseconds: i64,
    task_id: &'r str,
    endpoint_name: &'r str,
    host: &'r str,
    port: u16,
    model: &'r str,
    wire_format: &'static str,
    method: &'static str,
    path: &'r str,
    headers: &'r [(String, String)],
    /// The request body exactly as sent; every wire format sends UTF-8 JSON.
    body: &'r str,
    body_size: u64,
    body_sha256: String,
    pseudonymisation: Vec<PseudonymisationLine>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename = "outcome")]
struct OutcomeLine<'r> {
    line_time_milliseconds: i64,
    outbound_line: u64,
    bytes_written: &'static str,
    http_status: Option<u16>,
    provider_request_id: Option<&'r str>,
    /// The reply body when it is UTF-8, which every accepted reply is.
    reply_body: Option<&'r str>,
    /// The reply body as lowercase hex when it is not UTF-8.
    reply_body_hex: Option<String>,
    reply_size: u64,
    reply_sha256: String,
    charge: ChargeLine,
    error_kind: Option<&'static str>,
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn milliseconds(time: SystemTime) -> Result<i64, RecorderError> {
    let elapsed = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| RecorderError::new("the clock is before 1970"))?;
    i64::try_from(elapsed.as_millis())
        .map_err(|_| RecorderError::new("the clock is too far in the future"))
}

fn wire_format_name(format: WireFormat) -> &'static str {
    match format {
        WireFormat::OpenaiChat => "openai_chat",
        WireFormat::AnthropicMessages => "anthropic_messages",
    }
}

fn bytes_written_name(written: BytesWritten) -> &'static str {
    match written {
        BytesWritten::None => "none",
        BytesWritten::Some => "some",
        BytesWritten::All => "all",
    }
}

impl OutboundRecorder for Transcript {
    fn record_outbound(
        &self,
        record: OutboundRecord,
    ) -> impl Future<Output = Result<OutboundRecordId, RecorderError>> + Send {
        let transcript = self.clone();
        async move {
            let body = std::str::from_utf8(&record.body)
                .map_err(|_| RecorderError::new("the request body is not UTF-8"))?;
            let line = OutboundLine {
                line_time_milliseconds: milliseconds(record.time)?,
                task_id: &record.task_id,
                endpoint_name: &record.endpoint_name,
                host: &record.host,
                port: record.port,
                model: &record.model,
                wire_format: wire_format_name(record.wire_format),
                method: record.method,
                path: &record.path,
                headers: &record.headers,
                body,
                body_size: record.body_size,
                body_sha256: hex(&record.body_sha256),
                pseudonymisation: record
                    .pseudonymisation
                    .iter()
                    .map(|report| PseudonymisationLine {
                        replaced: report.replaced.clone(),
                        passed_through: report.passed_through.clone(),
                    })
                    .collect(),
            };
            let number = transcript
                .write_line(&line)
                .map_err(|error| RecorderError::new(error.to_string()))?;
            Ok(OutboundRecordId(number))
        }
    }

    fn record_outcome(
        &self,
        outcome: OutcomeRecord,
    ) -> impl Future<Output = Result<(), RecorderError>> + Send {
        let transcript = self.clone();
        async move {
            let utf8_body = std::str::from_utf8(&outcome.reply_body).ok();
            let line = OutcomeLine {
                line_time_milliseconds: milliseconds(outcome.time)?,
                outbound_line: outcome.outbound.0,
                bytes_written: bytes_written_name(outcome.bytes_written),
                http_status: outcome.http_status,
                provider_request_id: outcome.provider_request_id.as_deref(),
                reply_body: utf8_body,
                reply_body_hex: match utf8_body {
                    Some(_) => None,
                    None => Some(hex(&outcome.reply_body)),
                },
                reply_size: outcome.reply_size,
                reply_sha256: hex(&outcome.reply_sha256),
                charge: ChargeLine::from(outcome.usage_charged),
                error_kind: outcome.error_kind,
            };
            transcript
                .write_line(&line)
                .map(|_line_number| ())
                .map_err(|error| RecorderError::new(error.to_string()))
        }
    }
}
