use std::time::SystemTime;

use bytes::Bytes;
use snet_cloud::{
    BytesWritten, Charge, OutboundRecord, OutboundRecordId, OutboundRecorder, OutcomeRecord,
    WireFormat,
};

use crate::{TaskRecord, TaskRefusedLine, Transcript, TranscriptError};

fn lines(transcript: &Transcript) -> Vec<serde_json::Value> {
    std::fs::read_to_string(transcript.path())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn outbound(body: &'static [u8]) -> OutboundRecord {
    OutboundRecord {
        time: SystemTime::now(),
        task_id: "task-1".to_string(),
        endpoint_name: "anthropic".to_string(),
        host: "api.anthropic.com".to_string(),
        port: 443,
        model: "some-model".to_string(),
        wire_format: WireFormat::AnthropicMessages,
        configuration_digest: [7; 32],
        method: "POST",
        path: "/v1/messages".to_string(),
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        body: Bytes::from_static(body),
        body_size: body.len() as u64,
        body_sha256: [9; 32],
        pseudonymisation: Vec::new(),
    }
}

#[test]
fn the_file_is_new_and_private() {
    let home = tempfile::tempdir().unwrap();
    let transcript = Transcript::create(home.path(), "task-1").unwrap();
    assert_eq!(
        transcript.path(),
        home.path().join("transcripts").join("task-1.jsonl")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(transcript.path()), 0o600);
        assert_eq!(mode(&home.path().join("transcripts")), 0o700);
    }
    assert!(matches!(
        Transcript::create(home.path(), "task-1"),
        Err(TranscriptError::Io { .. })
    ));
}

#[test]
fn task_ids_that_could_leave_the_directory_are_refused() {
    let home = tempfile::tempdir().unwrap();
    for task_id in ["", "../escape", "a/b", "with space", &"x".repeat(129)] {
        assert!(matches!(
            Transcript::create(home.path(), task_id),
            Err(TranscriptError::TaskIdInvalid { .. })
        ));
    }
}

#[test]
fn task_lines_carry_their_kind_and_count_from_one() {
    let home = tempfile::tempdir().unwrap();
    let transcript = Transcript::create(home.path(), "task-1").unwrap();
    let refused = TaskRecord::TaskRefused(TaskRefusedLine {
        client_time_milliseconds: 1,
        task_id: None,
        field: "question".to_string(),
    });
    assert_eq!(transcript.record_task(&refused).unwrap(), 1);
    assert_eq!(transcript.record_task(&refused).unwrap(), 2);
    let written = lines(&transcript);
    assert_eq!(written[0]["kind"], "task_refused");
    assert_eq!(written[1]["field"], "question");
}

#[tokio::test(flavor = "current_thread")]
async fn a_request_is_written_exactly_as_sent_before_its_outcome() {
    let home = tempfile::tempdir().unwrap();
    let transcript = Transcript::create(home.path(), "task-1").unwrap();
    let body = br#"{"messages":[{"role":"user","content":"[EMAIL_1] asked"}]}"#;
    let id = transcript.record_outbound(outbound(body)).await.unwrap();
    assert_eq!(id, OutboundRecordId(1));
    transcript
        .record_outcome(OutcomeRecord {
            outbound: id,
            time: SystemTime::now(),
            bytes_written: BytesWritten::All,
            http_status: Some(200),
            provider_request_id: Some("req_1".to_string()),
            reply_body: Bytes::from_static(b"\xff\x00"),
            reply_size: 2,
            reply_sha256: [1; 32],
            usage_charged: Charge {
                requests: 1,
                bytes_sent: 60,
                input_tokens: 20,
                output_tokens: 5,
                cost_micro: 0,
            },
            error_kind: Some("reply_malformed"),
        })
        .await
        .unwrap();
    let written = lines(&transcript);
    assert_eq!(written[0]["kind"], "outbound");
    assert_eq!(written[0]["body"], std::str::from_utf8(body).unwrap());
    assert_eq!(written[0]["wire_format"], "anthropic_messages");
    assert_eq!(written[1]["kind"], "outcome");
    assert_eq!(written[1]["outbound_line"], 1);
    assert_eq!(written[1]["reply_body"], serde_json::Value::Null);
    assert_eq!(written[1]["reply_body_hex"], "ff00");
    assert_eq!(written[1]["charge"]["input_tokens"], "20");
}

#[tokio::test(flavor = "current_thread")]
async fn a_body_that_is_not_utf8_is_refused_and_not_written() {
    let home = tempfile::tempdir().unwrap();
    let transcript = Transcript::create(home.path(), "task-1").unwrap();
    assert!(transcript.record_outbound(outbound(b"\xff")).await.is_err());
    assert!(lines(&transcript).is_empty());
}
