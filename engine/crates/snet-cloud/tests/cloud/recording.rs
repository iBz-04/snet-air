use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use snet_cloud::{
    BytesWritten, Charge, CloudClient, CloudSwitch, EndpointConfig, Failure, RecordStage,
    TaskBudget, WireFormat,
};

use crate::support::certificates::TestPki;
use crate::support::recorder::{Mode, TestRecorder};
use crate::support::server::{Behaviour, json_response};
use crate::support::{
    Harness, HarnessOptions, IdentityChoice, KEY_MARKER, NO_DETECTION_POLICY, anthropic_reply,
    endpoint_text, key, outbound, outbound_with, request, test_destination,
};

fn with_recorder(
    mut options: HarnessOptions,
    configure: impl FnOnce(TestRecorder, &crate::support::HarnessParts) -> TestRecorder + 'static,
) -> HarnessOptions {
    options.recorder = Box::new(configure);
    options
}

/// The worst case of the standard request, read from what the budget holds
/// while a reservation is kept.
fn charge_of_kept_reservation(harness: &Harness) -> Charge {
    harness.recorder.outcomes()[0].usage_charged
}

#[tokio::test(flavor = "multi_thread")]
async fn with_the_switch_off_nothing_is_reserved_recorded_or_sent() {
    let harness = Harness::start(HarnessOptions::anthropic_ok()).await;
    harness.switch.turn_off();
    assert!(matches!(harness.send_failure().await, Failure::SwitchOff));
    assert!(harness.recorder.outbound().is_empty());
    assert!(harness.recorder.outcomes().is_empty());
    assert_eq!(harness.server.observations.connections(), 0);
    assert_eq!(harness.budget.charged().unwrap(), Charge::default());

    harness.switch.turn_on();
    assert!(harness.send().await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_switch_turned_off_after_recording_refuses_with_no_bytes_written() {
    let options = with_recorder(HarnessOptions::anthropic_ok(), |recorder, parts| {
        recorder.turning_off(&parts.switch)
    });
    let harness = Harness::start(options).await;
    assert!(matches!(harness.send_failure().await, Failure::SwitchOff));
    assert_eq!(harness.server.observations.connections(), 0);
    let outcomes = harness.recorder.outcomes();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].bytes_written, BytesWritten::None);
    assert_eq!(outcomes[0].error_kind, Some("switch_off"));
    assert_eq!(harness.budget.charged().unwrap(), outcomes[0].usage_charged);
    assert_eq!(outcomes[0].usage_charged.requests, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn turning_the_switch_off_aborts_an_exchange_in_flight_and_records_it() {
    let mut options = HarnessOptions::anthropic_ok();
    options.behaviour = Behaviour::WriteThenStall(Vec::new());
    let harness = Harness::start(options).await;
    let switch = harness.switch.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        switch.turn_off();
    });
    assert!(matches!(
        harness.send_failure().await,
        Failure::SwitchAborted
    ));
    let outcome = &harness.recorder.outcomes()[0];
    assert_eq!(outcome.bytes_written, BytesWritten::All);
    assert_eq!(outcome.http_status, None);
    assert_eq!(outcome.error_kind, Some("switch_aborted"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_outbound_record_arrives_before_any_connection_and_matches_what_was_sent() {
    let options = with_recorder(HarnessOptions::anthropic_ok(), |recorder, parts| {
        recorder.watching(&parts.server)
    });
    let harness = Harness::start(options).await;
    let reply = harness.send().await.unwrap();
    assert_eq!(
        *harness.recorder.connections_at_outbound.lock().unwrap(),
        vec![0]
    );
    assert_eq!(harness.server.observations.connections(), 1);

    let outbound = &harness.recorder.outbound()[0];
    let received = &harness.server.observations.requests()[0];
    assert_eq!(outbound.body.as_ref(), received.body.as_slice());
    assert_eq!(outbound.body_size, received.body.len() as u64);
    let digest: [u8; 32] = Sha256::digest(&received.body).into();
    assert_eq!(outbound.body_sha256, digest);
    assert_eq!(outbound.task_id, "task-1");
    assert_eq!(outbound.endpoint_name, "test-endpoint");
    assert_eq!(outbound.host, "127.0.0.1");
    assert_eq!(outbound.port, harness.server.port);
    assert_eq!(outbound.model, "test-model");
    assert_eq!(outbound.wire_format, WireFormat::AnthropicMessages);
    assert_eq!(
        outbound.configuration_digest,
        harness.client.config().digest()
    );
    assert_eq!(outbound.method, "POST");
    assert_eq!(outbound.path, "/v1/messages");
    let names: Vec<&str> = outbound
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
            "anthropic-version",
            "connection"
        ]
    );

    assert_eq!(outbound.pseudonymisation.len(), 3);
    assert!(
        outbound
            .pseudonymisation
            .iter()
            .all(|report| report.destination == test_destination())
    );
    assert_eq!(reply.turn.destination(), &test_destination());
    assert_eq!(reply.turn.as_str(), r#"{"answer":"the inner seal"}"#);

    let outcome = &harness.recorder.outcomes()[0];
    assert_eq!(outcome.outbound, snet_cloud::OutboundRecordId(1));
    assert_eq!(outcome.reply_body.as_ref(), anthropic_reply(30).as_bytes());
    assert_eq!(outcome.usage_charged, reply.charged);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_accepted_reply_replaces_the_reservation_with_its_usage() {
    let harness = Harness::start(HarnessOptions::anthropic_ok()).await;
    let reply = harness.send().await.unwrap();
    assert_eq!(reply.usage.input_tokens, 100);
    assert_eq!(reply.usage.output_tokens, 30);
    // 100 input tokens at 5 USD and 30 output tokens at 25 USD per million.
    assert_eq!(reply.charged.cost_micro, 500 + 750);
    assert_eq!(harness.budget.charged().unwrap(), reply.charged);
}

#[tokio::test(flavor = "multi_thread")]
async fn usage_above_the_worst_case_is_charged_and_closes_the_budget() {
    let harness = Harness::start(HarnessOptions::replying(
        WireFormat::AnthropicMessages,
        json_response(&anthropic_reply(5_000)),
    ))
    .await;
    assert!(matches!(
        harness.send_failure().await,
        Failure::UsageAboveWorstCase
    ));
    assert!(harness.budget.is_closed().unwrap());
    assert_eq!(harness.budget.charged().unwrap().output_tokens, 5_000);
    match harness.send_failure().await {
        Failure::Budget(snet_cloud::BudgetError::Closed) => {}
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_outbound_record_stops_the_request_and_releases_the_reservation() {
    let options = with_recorder(HarnessOptions::anthropic_ok(), |recorder, _parts| {
        recorder.outbound_mode(Mode::Fail)
    });
    let harness = Harness::start(options).await;
    match harness.send_failure().await {
        Failure::RecorderFailed { stage, .. } => assert_eq!(stage, RecordStage::Outbound),
        other => panic!("{other:?}"),
    }
    assert_eq!(harness.server.observations.connections(), 0);
    assert_eq!(harness.budget.charged().unwrap(), Charge::default());
    assert!(harness.recorder.outcomes().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_outcome_record_discards_the_reply() {
    let options = with_recorder(HarnessOptions::anthropic_ok(), |recorder, _parts| {
        recorder.outcome_mode(Mode::Fail)
    });
    let harness = Harness::start(options).await;
    match harness.send_failure().await {
        Failure::RecorderFailed { stage, .. } => assert_eq!(stage, RecordStage::Outcome),
        other => panic!("{other:?}"),
    }
    assert_eq!(harness.server.observations.connections(), 1);
    assert_eq!(harness.budget.charged().unwrap().output_tokens, 30);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_caller_that_stops_waiting_still_gets_an_outcome_and_keeps_the_reservation() {
    let mut options = HarnessOptions::anthropic_ok();
    options.behaviour = Behaviour::RespondAfter(
        Duration::from_millis(600),
        json_response(&anthropic_reply(30)),
    );
    let harness = Harness::start(options).await;
    let abandoned = tokio::time::timeout(Duration::from_millis(150), harness.send()).await;
    assert!(abandoned.is_err(), "the caller should have stopped waiting");

    let outcomes = harness.recorder.wait_for_outcomes(1).await;
    assert_eq!(outcomes[0].error_kind, Some("abandoned"));
    assert_eq!(outcomes[0].bytes_written, BytesWritten::All);
    assert_eq!(outcomes[0].http_status, Some(200));
    let kept = charge_of_kept_reservation(&harness);
    assert_eq!(kept.output_tokens, 1000);
    assert_eq!(harness.budget.charged().unwrap(), kept);
}

#[tokio::test(flavor = "multi_thread")]
async fn timeouts_and_tls_failures_each_get_an_outcome() {
    let mut options = HarnessOptions::anthropic_ok();
    options.identity = IdentityChoice::Untrusted;
    let harness = Harness::start(options).await;
    assert!(matches!(harness.send_failure().await, Failure::Tls { .. }));
    assert_eq!(harness.recorder.outcomes().len(), 1);
    assert_eq!(harness.recorder.outbound().len(), 1);
}

/// A client whose exchange never touches the network, for tests on paused
/// time: the recorder hangs, or the switch is already off when the exchange
/// task starts.
fn offline_client(
    recorder: TestRecorder,
    switch: &CloudSwitch,
) -> (CloudClient<TestRecorder>, TaskBudget, Arc<TestRecorder>) {
    let pki = TestPki::new().unwrap();
    let config = EndpointConfig::parse(
        &endpoint_text(WireFormat::AnthropicMessages, 9),
        &snet_testkit::certificates::read_from(pki.directory.path()),
    )
    .unwrap();
    let budget = TaskBudget::new("task-1", config.task_limits()).unwrap();
    let recorder = Arc::new(recorder);
    let client = CloudClient::new(config, key(), Arc::clone(&recorder), switch.clone()).unwrap();
    (client, budget, recorder)
}

#[tokio::test(start_paused = true)]
async fn a_recorder_that_never_returns_on_the_outbound_record_hits_the_30_second_limit() {
    let switch = CloudSwitch::new(true);
    let (client, budget, _recorder) =
        offline_client(TestRecorder::new().outbound_mode(Mode::Hang), &switch);
    let started = tokio::time::Instant::now();
    let error = client.send(&budget, request()).await.unwrap_err();
    assert!(started.elapsed() >= Duration::from_secs(30));
    match error.failure {
        Failure::RecorderTimedOut { stage } => assert_eq!(stage, RecordStage::Outbound),
        other => panic!("{other:?}"),
    }
    assert_eq!(budget.charged().unwrap(), Charge::default());
}

#[tokio::test(start_paused = true)]
async fn a_recorder_that_never_returns_on_the_outcome_hits_the_30_second_limit() {
    let switch = CloudSwitch::new(true);
    let recorder = TestRecorder::new()
        .outcome_mode(Mode::Hang)
        .turning_off(&switch);
    let (client, budget, recorder) = offline_client(recorder, &switch);
    let started = tokio::time::Instant::now();
    let error = client.send(&budget, request()).await.unwrap_err();
    assert!(started.elapsed() >= Duration::from_secs(30));
    match error.failure {
        Failure::RecorderTimedOut { stage } => assert_eq!(stage, RecordStage::Outcome),
        other => panic!("{other:?}"),
    }
    assert_eq!(recorder.outbound().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_key_appears_in_no_record_error_or_debug_output() {
    let mut texts = Vec::new();
    let error_reply = crate::support::server::http_response(
        401,
        &[("content-type", "application/json")],
        br#"{"type":"error","error":{"type":"authentication_error","message":"bad key"}}"#,
    );
    let mut untrusted = HarnessOptions::anthropic_ok();
    untrusted.identity = IdentityChoice::Untrusted;
    for options in [
        HarnessOptions::anthropic_ok(),
        HarnessOptions::replying(
            WireFormat::OpenaiChat,
            json_response(&crate::support::openai_reply()),
        ),
        HarnessOptions::replying(WireFormat::AnthropicMessages, error_reply),
        untrusted,
    ] {
        let harness = Harness::start(options).await;
        match harness.send().await {
            Ok(reply) => texts.push(format!("{reply:?}")),
            Err(error) => {
                texts.push(error.to_string());
                texts.push(format!("{error:?}"));
            }
        }
        for record in harness.recorder.outbound() {
            texts.push(format!("{record:?}"));
            texts.push(String::from_utf8_lossy(&record.body).into_owned());
        }
        for outcome in harness.recorder.outcomes() {
            texts.push(format!("{outcome:?}"));
        }
        texts.push(format!("{:?}", harness.client.config()));
        texts.push(format!("{:?}", key()));
    }
    assert!(texts.len() > 10);
    let leaks = texts
        .iter()
        .filter(|text| text.contains(KEY_MARKER))
        .count();
    assert_eq!(leaks, 0, "the key appeared in {leaks} texts");
}

#[tokio::test(flavor = "multi_thread")]
async fn texts_and_replies_of_another_endpoint_or_policy_are_refused_before_recording() {
    let harness = Harness::start(HarnessOptions::anthropic_ok()).await;
    let first = harness.send().await.unwrap();

    let elsewhere = snet_cloud::Destination::new("other-endpoint", "127.0.0.1");
    let mut foreign_text = request();
    foreign_text.conversation = vec![snet_cloud::CloudTurn::User(outbound_with(
        NO_DETECTION_POLICY,
        elsewhere,
        "Which seal?",
    ))];
    let other_policy = NO_DETECTION_POLICY.replace("EMAIL = false", "EMAIL = true");
    let mut foreign_policy = request();
    foreign_policy.instructions = outbound_with(&other_policy, test_destination(), "Answer.");
    let mut with_reply = request();
    with_reply
        .conversation
        .push(snet_cloud::CloudTurn::Assistant(first.turn.clone()));
    with_reply
        .conversation
        .push(snet_cloud::CloudTurn::User(outbound("And pump 4?")));

    let other_harness = Harness::start(HarnessOptions::anthropic_ok()).await;
    let mut foreign_reply = request();
    foreign_reply
        .conversation
        .push(snet_cloud::CloudTurn::Assistant(first.turn.clone()));
    foreign_reply
        .conversation
        .push(snet_cloud::CloudTurn::User(outbound("And pump 4?")));
    let other_client_with_renamed_endpoint = {
        let pki = TestPki::new().unwrap();
        let text = endpoint_text(WireFormat::AnthropicMessages, other_harness.server.port)
            .replace("name = \"test-endpoint\"", "name = \"renamed\"");
        let config = EndpointConfig::parse(
            &text,
            &snet_testkit::certificates::read_from(pki.directory.path()),
        )
        .unwrap();
        let budget = TaskBudget::new("task-2", config.task_limits()).unwrap();
        let recorder = Arc::new(TestRecorder::new());
        let client =
            CloudClient::new(config, key(), Arc::clone(&recorder), CloudSwitch::new(true)).unwrap();
        (client, budget, recorder)
    };

    for (request, expected) in [
        (foreign_text, "destination_mismatch"),
        (foreign_policy, "policy_mismatch"),
    ] {
        let error = harness
            .client
            .send(&harness.budget, request)
            .await
            .unwrap_err();
        assert_eq!(error.failure.kind(), expected);
    }
    let (client, budget, recorder) = other_client_with_renamed_endpoint;
    let error = client.send(&budget, foreign_reply).await.unwrap_err();
    assert_eq!(error.failure.kind(), "destination_mismatch");
    assert!(recorder.outbound().is_empty());
    assert_eq!(harness.recorder.outbound().len(), 1);

    harness
        .client
        .send(&harness.budget, with_reply)
        .await
        .unwrap();
    let body: serde_json::Value =
        serde_json::from_slice(&harness.recorder.outbound()[1].body).unwrap();
    assert_eq!(body["messages"][1]["role"], "assistant");
    assert_eq!(
        body["messages"][1]["content"],
        r#"{"answer":"the inner seal"}"#
    );
}

/// Reports on for its first `on_checks` checks and off from then on.
struct CountdownSource {
    on_checks: usize,
    checks: std::sync::atomic::AtomicUsize,
}

impl snet_cloud::SwitchSource for CountdownSource {
    fn reports_on(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
        let check = self
            .checks
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move { check < self.on_checks })
    }
}

fn with_source(on_checks: usize) -> HarnessOptions {
    let mut options = HarnessOptions::anthropic_ok();
    options.switch_source = Some(Arc::new(CountdownSource {
        on_checks,
        checks: std::sync::atomic::AtomicUsize::new(0),
    }));
    options
}

#[tokio::test(flavor = "multi_thread")]
async fn a_source_reporting_off_refuses_before_anything_is_recorded() {
    let harness = Harness::start(with_source(0)).await;
    assert!(matches!(harness.send_failure().await, Failure::SwitchOff));
    assert!(harness.recorder.outbound().is_empty());
    assert_eq!(harness.server.observations.connections(), 0);
    assert!(!harness.switch.is_on(), "the switch stays off");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_source_reporting_off_at_a_later_check_writes_no_request_byte() {
    for on_checks in [1, 2] {
        let harness = Harness::start(with_source(on_checks)).await;
        assert!(matches!(harness.send_failure().await, Failure::SwitchOff));
        let outcomes = harness.recorder.outcomes();
        assert_eq!(outcomes.len(), 1, "after {on_checks} checks");
        assert_eq!(outcomes[0].bytes_written, BytesWritten::None);
        assert_eq!(outcomes[0].error_kind, Some("switch_off"));
        assert!(harness.server.observations.requests().is_empty());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn exchanges_finished_waits_for_an_abandoned_exchange_to_record_its_outcome() {
    let mut options = HarnessOptions::anthropic_ok();
    options.behaviour = Behaviour::WriteThenStall(Vec::new());
    let harness = Harness::start(options).await;
    let abandoned = tokio::time::timeout(Duration::from_millis(300), harness.send()).await;
    assert!(abandoned.is_err(), "the caller stopped waiting");
    assert!(harness.recorder.outcomes().is_empty());
    harness.switch.turn_off();
    tokio::time::timeout(Duration::from_secs(10), harness.client.exchanges_finished())
        .await
        .unwrap();
    let outcomes = harness.recorder.outcomes();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].error_kind, Some("switch_aborted"));
}
