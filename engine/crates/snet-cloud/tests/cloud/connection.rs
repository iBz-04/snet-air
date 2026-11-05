use std::process::Command;
use std::time::{Duration, Instant};

use rustls::{HandshakeKind, NamedGroup, ProtocolVersion};
use snet_cloud::{BytesWritten, Failure, WireFormat};

use crate::support::server::{Behaviour, TestServer, TlsVersion, http_response, json_response};
use crate::support::{Harness, HarnessOptions, IdentityChoice, set};

#[tokio::test(flavor = "multi_thread")]
async fn tls_1_3_with_the_hybrid_group_and_http_1_1_on_a_new_connection_per_request() {
    let harness = Harness::start(HarnessOptions::anthropic_ok()).await;
    let first = harness.send().await.unwrap();
    let second = harness.send().await.unwrap();
    assert_eq!(first.model, "test-model-served");
    assert_eq!(second.provider_request_id.as_deref(), Some("req_test_1"));

    assert_eq!(harness.server.observations.connections(), 2);
    let handshakes = harness.server.observations.handshakes();
    assert_eq!(handshakes.len(), 2);
    for facts in handshakes {
        assert_eq!(facts.version, Some(ProtocolVersion::TLSv1_3));
        assert_eq!(facts.group, Some(NamedGroup::X25519MLKEM768));
        assert_eq!(facts.alpn.as_deref(), Some(b"http/1.1".as_slice()));
        // A full handshake on the second connection: no resumption attempt.
        assert_eq!(facts.kind, Some(HandshakeKind::Full));
    }
}

async fn assert_tls_refused(options: HarnessOptions) {
    let harness = Harness::start(options).await;
    match harness.send_failure().await {
        Failure::Tls { cause } => assert!(!cause.is_empty()),
        other => panic!("expected a TLS failure, got {other:?}"),
    }
    assert_eq!(harness.server.observations.request_bytes(), 0);
    assert!(harness.server.observations.requests().is_empty());
    let outcomes = harness.recorder.outcomes();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].bytes_written, BytesWritten::None);
    assert_eq!(outcomes[0].error_kind, Some("tls"));
}

#[tokio::test(flavor = "multi_thread")]
async fn another_name_an_expired_certificate_and_an_untrusted_ca_each_fail_before_any_request_byte()
{
    for identity in [
        IdentityChoice::OtherName,
        IdentityChoice::Expired,
        IdentityChoice::Untrusted,
    ] {
        let mut options = HarnessOptions::anthropic_ok();
        options.identity = identity;
        assert_tls_refused(options).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tls_1_2_only_server_is_refused() {
    let mut options = HarnessOptions::anthropic_ok();
    options.tls = TlsVersion::Tls12Only;
    assert_tls_refused(options).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_public_roots_alone_do_not_trust_the_site_ca() {
    let mut options = HarnessOptions::anthropic_ok();
    options.edit = Box::new(|text| {
        let text = set(&text, "extra_ca_certificates", "[]");
        set(&text, "replace_public_roots", "false")
    });
    assert_tls_refused(options).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_redirect_is_an_error_and_its_target_sees_no_connection() {
    let (target_port, target) = TestServer::counting_listener().await.unwrap();
    let location = format!("https://127.0.0.1:{target_port}/v1/messages");
    let redirect = http_response(302, &[("location", location.as_str())], b"");
    let harness = Harness::start(HarnessOptions::replying(
        WireFormat::AnthropicMessages,
        redirect,
    ))
    .await;
    assert!(matches!(
        harness.send_failure().await,
        Failure::Redirect { status: 302 }
    ));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(target.connections(), 0);
    let outcomes = harness.recorder.outcomes();
    assert_eq!(outcomes[0].http_status, Some(302));
    assert_eq!(outcomes[0].error_kind, Some("redirect"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_loopback_endpoint_is_refused_unless_private_addresses_are_allowed() {
    let mut options = HarnessOptions::anthropic_ok();
    options.edit = Box::new(|text| set(&text, "allow_private_addresses", "false"));
    let harness = Harness::start(options).await;
    assert!(matches!(
        harness.send_failure().await,
        Failure::NoPermittedAddress { .. }
    ));
    assert_eq!(harness.server.observations.connections(), 0);

    let mut options = HarnessOptions::anthropic_ok();
    options.edit = Box::new(|text| {
        let text = set(&text, "allow_private_addresses", "false");
        text.replace("https://127.0.0.1:", "https://localhost:")
    });
    let harness = Harness::start(options).await;
    let for_localhost =
        crate::support::request_for(snet_cloud::Destination::new("test-endpoint", "localhost"));
    let failure = harness
        .client
        .send(&harness.budget, for_localhost)
        .await
        .unwrap_err()
        .failure;
    assert!(
        matches!(failure, Failure::NoPermittedAddress { .. }),
        "{failure}"
    );

    let served = Harness::start(HarnessOptions::anthropic_ok()).await;
    assert!(served.send().await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_that_never_completes_the_handshake_hits_the_connect_timeout() {
    let mut options = HarnessOptions::anthropic_ok();
    options.behaviour = Behaviour::NeverHandshake;
    options.edit = Box::new(|text| set(&text, "connect_timeout_seconds", "1"));
    let harness = Harness::start(options).await;
    let started = Instant::now();
    assert!(matches!(
        harness.send_failure().await,
        Failure::ConnectTimeout
    ));
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_millis(900) && elapsed < Duration::from_secs(5),
        "{elapsed:?}"
    );
    assert_eq!(
        harness.recorder.outcomes()[0].bytes_written,
        BytesWritten::None
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_that_stalls_mid_body_hits_the_total_timeout() {
    let mut head_and_part =
        b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 100\r\n\r\n"
            .to_vec();
    head_and_part.extend_from_slice(b"{\"id\":");
    let mut options = HarnessOptions::anthropic_ok();
    options.behaviour = Behaviour::WriteThenStall(head_and_part);
    options.edit = Box::new(|text| set(&text, "total_timeout_seconds", "2"));
    let harness = Harness::start(options).await;
    let started = Instant::now();
    assert!(matches!(
        harness.send_failure().await,
        Failure::TotalTimeout
    ));
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_millis(1900) && elapsed < Duration::from_secs(6),
        "{elapsed:?}"
    );
    let outcome = &harness.recorder.outcomes()[0];
    assert_eq!(outcome.bytes_written, BytesWritten::All);
    assert_eq!(outcome.http_status, Some(200));
    assert_eq!(outcome.reply_body.as_ref(), b"{\"id\":");
    assert_eq!(outcome.error_kind, Some("total_timeout"));
}

const PROXY_PORT_VARIABLE: &str = "SNET_CLOUD_TEST_PROXY_PORT";

/// Runs in a child process whose environment names a listening proxy.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "run by proxy_environment_variables_are_never_used in a child process"]
async fn child_sends_one_request_with_proxy_variables_set() {
    assert!(std::env::var(PROXY_PORT_VARIABLE).is_ok());
    let harness = Harness::start(HarnessOptions::anthropic_ok()).await;
    harness.send().await.unwrap();
    assert_eq!(harness.server.observations.connections(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn proxy_environment_variables_are_never_used() {
    let (proxy_port, proxy) = TestServer::counting_listener().await.unwrap();
    let proxy_url = format!("http://127.0.0.1:{proxy_port}");
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "connection::child_sends_one_request_with_proxy_variables_set",
            "--test-threads",
            "1",
        ])
        .env(PROXY_PORT_VARIABLE, proxy_port.to_string())
        .env("HTTPS_PROXY", &proxy_url)
        .env("HTTP_PROXY", &proxy_url)
        .env("ALL_PROXY", &proxy_url)
        .env("https_proxy", &proxy_url)
        .env("http_proxy", &proxy_url)
        .env("all_proxy", &proxy_url)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("1 passed"), "{stdout}");
    assert_eq!(proxy.connections(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_openai_chat_endpoint_round_trips() {
    let harness = Harness::start(HarnessOptions::replying(
        WireFormat::OpenaiChat,
        json_response(&crate::support::openai_reply()),
    ))
    .await;
    let reply = harness.send().await.unwrap();
    assert_eq!(
        reply.object["answer"],
        snet_cloud::JsonValue::String("the inner seal".to_string())
    );
    let request = &harness.server.observations.requests()[0];
    assert_eq!(request.request_line, "POST /v1/chat/completions HTTP/1.1");
}
