use snet_cloud::{BytesWritten, Charge, Failure, WireFormat};

use crate::support::server::{http_response, json_response};
use crate::support::{Harness, HarnessOptions, KEY_MARKER, anthropic_reply, openai_reply, set};

fn header<'a>(headers: &'a [(String, String)], name: &str) -> &'a str {
    headers
        .iter()
        .find(|(header_name, _)| header_name == name)
        .map(|(_, value)| value.as_str())
        .unwrap_or_else(|| panic!("no {name} header"))
}

fn editing(mut options: HarnessOptions, key: &'static str, value: String) -> HarnessOptions {
    options.edit = Box::new(move |text| set(&text, key, &value));
    options
}

#[tokio::test(flavor = "multi_thread")]
async fn each_format_sends_exactly_the_headers_of_cld_13() {
    let cases = [
        (
            WireFormat::AnthropicMessages,
            json_response(&anthropic_reply(30)),
            "POST /v1/messages HTTP/1.1",
            vec![
                "host",
                "content-type",
                "content-length",
                "authorization",
                "anthropic-version",
                "connection",
            ],
        ),
        (
            WireFormat::OpenaiChat,
            json_response(&openai_reply()),
            "POST /v1/chat/completions HTTP/1.1",
            vec![
                "host",
                "content-type",
                "content-length",
                "authorization",
                "connection",
            ],
        ),
    ];
    for (format, response, request_line, expected_names) in cases {
        let harness = Harness::start(HarnessOptions::replying(format, response)).await;
        harness.send().await.unwrap();
        let received = &harness.server.observations.requests()[0];
        assert_eq!(received.request_line, request_line);
        let names: Vec<&str> = received
            .headers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, expected_names);

        let port = harness.server.port;
        assert_eq!(
            header(&received.headers, "host"),
            format!("127.0.0.1:{port}")
        );
        assert_eq!(
            header(&received.headers, "content-type"),
            "application/json"
        );
        assert_eq!(
            header(&received.headers, "content-length"),
            received.body.len().to_string()
        );
        assert_eq!(header(&received.headers, "connection"), "close");
        if format == WireFormat::AnthropicMessages {
            assert_eq!(header(&received.headers, "anthropic-version"), "2026-10-21");
        }
        let authorization_is_the_key = header(&received.headers, "authorization")
            == format!("Bearer sk-test-{KEY_MARKER}-0123456789");
        assert!(
            authorization_is_the_key,
            "the authorization header is not the bearer key"
        );

        // The recorded body is exactly the bytes received.
        let outbound = &harness.recorder.outbound()[0];
        assert_eq!(outbound.body.as_ref(), received.body.as_slice());
        let outcome = &harness.recorder.outcomes()[0];
        assert_eq!(outcome.bytes_written, BytesWritten::All);
        assert_eq!(outcome.http_status, Some(200));
        assert_eq!(outcome.provider_request_id.as_deref(), Some("req_test_1"));
        assert_eq!(outcome.error_kind, None);
        assert_eq!(
            harness.server.observations.request_bytes(),
            received.raw.len()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_named_workspace_reaches_the_server_after_the_version() {
    let mut options = HarnessOptions::replying(
        WireFormat::AnthropicMessages,
        json_response(&anthropic_reply(30)),
    );
    options.edit = Box::new(|text| {
        text.replace(
            "anthropic_workspace = \"key_scoped\"",
            "anthropic_workspace = \"wrkspc_01JwQvzr7rXLA5AGx3HKfFUJ\"",
        )
    });
    let harness = Harness::start(options).await;
    harness.send().await.unwrap();
    let received = &harness.server.observations.requests()[0];
    let names: Vec<&str> = received
        .headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "host",
            "content-type",
            "content-length",
            "authorization",
            "anthropic-version",
            "anthropic-workspace-id",
            "connection"
        ]
    );
    let workspace = received
        .headers
        .iter()
        .find(|(name, _)| name == "anthropic-workspace-id")
        .map(|(_, value)| value.as_str());
    assert_eq!(workspace, Some("wrkspc_01JwQvzr7rXLA5AGx3HKfFUJ"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_one_byte_over_its_cap_is_refused_before_recording() {
    let probe = Harness::start(HarnessOptions::anthropic_ok()).await;
    probe.send().await.unwrap();
    let body_size = probe.recorder.outbound()[0].body_size;

    let at_cap = Harness::start(editing(
        HarnessOptions::anthropic_ok(),
        "request_bytes_cap",
        body_size.to_string(),
    ))
    .await;
    assert!(at_cap.send().await.is_ok());

    let over_cap = Harness::start(editing(
        HarnessOptions::anthropic_ok(),
        "request_bytes_cap",
        (body_size - 1).to_string(),
    ))
    .await;
    assert!(matches!(
        over_cap.send_failure().await,
        Failure::RequestTooLarge { .. }
    ));
    assert!(over_cap.recorder.outbound().is_empty());
    assert_eq!(over_cap.server.observations.connections(), 0);
    assert_eq!(over_cap.budget.charged().unwrap(), Charge::default());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reply_one_byte_over_its_cap_is_an_error() {
    let body = anthropic_reply(30);
    let length = body.len();
    let at_cap = Harness::start(editing(
        HarnessOptions::anthropic_ok(),
        "reply_bytes_cap",
        length.to_string(),
    ))
    .await;
    assert!(at_cap.send().await.is_ok());

    let over_cap = Harness::start(editing(
        HarnessOptions::anthropic_ok(),
        "reply_bytes_cap",
        (length - 1).to_string(),
    ))
    .await;
    assert!(matches!(
        over_cap.send_failure().await,
        Failure::ReplyTooLarge { .. }
    ));
    let outcome = &over_cap.recorder.outcomes()[0];
    assert!(outcome.reply_size < length as u64);
    assert_eq!(outcome.error_kind, Some("reply_too_large"));
}

fn response_with_headers(total_headers: usize, padding: usize) -> Vec<u8> {
    let body = anthropic_reply(30);
    // json_response adds three headers and http_response two more.
    let mut headers: Vec<(String, String)> = vec![
        ("content-type".to_string(), "application/json".to_string()),
        ("request-id".to_string(), "req_test_1".to_string()),
    ];
    let filler = total_headers - headers.len() - 2;
    for index in 0..filler {
        headers.push((format!("x-filler-{index}"), "v".repeat(padding.max(1))));
    }
    let borrowed: Vec<(&str, &str)> = headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    http_response(200, &borrowed, body.as_bytes())
}

#[tokio::test(flavor = "multi_thread")]
async fn sixty_four_headers_are_read_and_sixty_five_are_refused() {
    let allowed = Harness::start(HarnessOptions::replying(
        WireFormat::AnthropicMessages,
        response_with_headers(64, 1),
    ))
    .await;
    assert!(allowed.send().await.is_ok());

    let refused = Harness::start(HarnessOptions::replying(
        WireFormat::AnthropicMessages,
        response_with_headers(65, 1),
    ))
    .await;
    assert!(matches!(
        refused.send_failure().await,
        Failure::ReplyHeadRefused { .. }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reply_head_over_65_536_bytes_is_refused() {
    let refused = Harness::start(HarnessOptions::replying(
        WireFormat::AnthropicMessages,
        response_with_headers(5, 70_000),
    ))
    .await;
    assert!(matches!(
        refused.send_failure().await,
        Failure::ReplyHeadRefused { .. }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn encoded_or_non_json_success_replies_are_refused() {
    let body = anthropic_reply(30);
    let gzip = http_response(
        200,
        &[
            ("content-type", "application/json"),
            ("content-encoding", "gzip"),
        ],
        body.as_bytes(),
    );
    let harness = Harness::start(HarnessOptions::replying(
        WireFormat::AnthropicMessages,
        gzip,
    ))
    .await;
    assert!(matches!(
        harness.send_failure().await,
        Failure::UnexpectedEncoding
    ));

    let html = http_response(200, &[("content-type", "text/html")], b"<html></html>");
    let harness = Harness::start(HarnessOptions::replying(
        WireFormat::AnthropicMessages,
        html,
    ))
    .await;
    assert!(matches!(
        harness.send_failure().await,
        Failure::UnexpectedContentType
    ));

    let identity = http_response(
        200,
        &[
            ("content-type", "Application/JSON; charset=utf-8"),
            ("content-encoding", "identity"),
        ],
        body.as_bytes(),
    );
    let harness = Harness::start(HarnessOptions::replying(
        WireFormat::AnthropicMessages,
        identity,
    ))
    .await;
    assert!(harness.send().await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn error_replies_keep_their_status_type_request_id_and_retryability() {
    let proxy_page = http_response(
        502,
        &[("content-type", "text/html"), ("request-id", "req_proxy")],
        b"<html>Bad Gateway</html>",
    );
    let harness = Harness::start(HarnessOptions::replying(
        WireFormat::AnthropicMessages,
        proxy_page,
    ))
    .await;
    match harness.send_failure().await {
        Failure::Provider {
            status,
            error_type,
            request_id,
            retryable,
            ..
        } => {
            assert_eq!(status, 502);
            assert_eq!(error_type, None);
            assert_eq!(request_id.as_deref(), Some("req_proxy"));
            assert!(retryable);
        }
        other => panic!("{other:?}"),
    }

    let overloaded = http_response(
        529,
        &[
            ("content-type", "application/json"),
            ("request-id", "req_busy"),
        ],
        br#"{"type":"error","error":{"type":"overloaded_error","message":"busy"}}"#,
    );
    let harness = Harness::start(HarnessOptions::replying(
        WireFormat::AnthropicMessages,
        overloaded,
    ))
    .await;
    match harness.send_failure().await {
        Failure::Provider {
            status,
            error_type,
            retryable,
            ..
        } => {
            assert_eq!(status, 529);
            assert_eq!(error_type.as_deref(), Some("overloaded_error"));
            assert!(retryable);
        }
        other => panic!("{other:?}"),
    }
    // A non-2xx reply keeps the reservation in full.
    let outcome = &harness.recorder.outcomes()[0];
    assert_eq!(harness.budget.charged().unwrap(), outcome.usage_charged);
    assert_eq!(outcome.error_kind, Some("provider"));
    assert_eq!(outcome.http_status, Some(529));
    assert_eq!(harness.server.observations.connections(), 1);
}
