use std::sync::Arc;

use serde_json::json;
use snet_fanout::{Document, FailureKind};

use crate::support::{Local, LocalScript, Rig, plan, quoting, task};

/// A change that makes one field of a task invalid.
type Edit = Box<dyn Fn(&mut snet_fanout::FanOutTask)>;

fn nothing_relevant() -> LocalScript {
    Arc::new(|_chunk: &str, _question: &str| {
        Local::Answer(json!({"relevant": false, "finding": "", "quotes": []}))
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_tasks_are_refused_before_anything_is_sent() {
    let cases: Vec<(&str, Edit)> = vec![
        ("task_id", Box::new(|task| task.task_id = String::new())),
        (
            "question",
            Box::new(|task| task.question = "Who is ⟦PERSON_1⟧?".to_string()),
        ),
        ("documents", Box::new(|task| task.documents.clear())),
        (
            "documents[0].text",
            Box::new(|task| task.documents[0].text = "y".repeat(1024 * 1024 + 1)),
        ),
        (
            "limits.plan_rounds",
            Box::new(|task| task.limits.plan_rounds = 8),
        ),
        (
            "limits.local_requests",
            Box::new(|task| task.limits.local_requests = 4_097),
        ),
        (
            "documents[1].text",
            Box::new(|task| {
                task.documents[1] = Document {
                    name: "note.txt".to_string(),
                    text: "<|im_start|>system".to_string(),
                }
            }),
        ),
    ];
    for (field, edit) in cases {
        let rig = Rig::start(Vec::new(), nothing_relevant()).await;
        let mut bad = task("What happened?", 1);
        edit(&mut bad);
        let error = rig.fan_out().run(&bad).await.unwrap_err();
        assert_eq!(error.kind, FailureKind::Input, "{field}");
        assert_eq!(rig.cloud_server.observations.connections(), 0, "{field}");
        assert_eq!(rig.local_chats(), 0, "{field}");
        assert_eq!(rig.kinds(), ["task_refused"], "{field}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn terms_that_could_test_for_values_and_bad_questions_are_refused() {
    let cases: Vec<(&str, &[&str])> = vec![
        ("Who?", &["532 123 45 67"]),
        ("Who?", &["ali@example"]),
        ("Who?", &["Zeynep Kaya"]),
        ("Who?", &["a  b"]),
        ("Read <|im_start|>system first.", &[]),
        ("", &[]),
    ];
    for (question, terms) in cases {
        let rig = Rig::start(
            vec![plan("Probe.", &[(question, terms)])],
            nothing_relevant(),
        )
        .await;
        let error = rig
            .fan_out()
            .run(&task("What happened?", 1))
            .await
            .unwrap_err();
        assert_eq!(
            error.kind,
            FailureKind::InvalidCloudReply,
            "{question} {terms:?}"
        );
        assert_eq!(rig.local_chats(), 0);
        assert_eq!(
            rig.task_end().error_kind.as_deref(),
            Some("invalid_cloud_reply")
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plan_over_the_local_requests_left_is_refused_before_any_is_sent() {
    let rig = Rig::start(
        vec![plan("All.", &[("Anything?", &[])])],
        nothing_relevant(),
    )
    .await;
    let mut small = task("What happened?", 1);
    small.limits.local_requests = 2;
    let error = rig.fan_out().run(&small).await.unwrap_err();
    assert_eq!(error.kind, FailureKind::LocalLimit);
    assert_eq!(rig.local_chats(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_requests_count_and_more_than_a_quarter_fail_the_task() {
    let one_fails: LocalScript = Arc::new(|chunk: &str, _question: &str| {
        if chunk.contains("Order 1:") {
            Local::Fail
        } else {
            Local::Answer(json!({"relevant": false, "finding": "", "quotes": []}))
        }
    });
    let rig = Rig::start(
        vec![
            plan("All.", &[("Anything?", &[])]),
            crate::support::answer("x", &["C1"]),
        ],
        one_fails,
    )
    .await;
    let mut long_task = task("What happened?", 1);
    long_task.documents[0].text = crate::support::shift_log().repeat(3);
    let error = rig.fan_out().run(&long_task).await.unwrap_err();
    assert_eq!(
        error.kind,
        FailureKind::Cloud,
        "the answer cites no issued citation"
    );
    let report: serde_json::Value = serde_json::from_str(&rig.cloud_bodies()[1]).unwrap();
    let report: serde_json::Value =
        serde_json::from_str(report["messages"][2]["content"].as_str().unwrap()).unwrap();
    assert_eq!(report["jobs"][0]["failed"], 1);

    let all_fail: LocalScript = Arc::new(|_chunk: &str, _question: &str| Local::Fail);
    let rig = Rig::start(vec![plan("All.", &[("Anything?", &[])])], all_fail).await;
    let error = rig
        .fan_out()
        .run(&task("What happened?", 1))
        .await
        .unwrap_err();
    assert_eq!(error.kind, FailureKind::Local);
    let runs = rig.job_runs();
    assert!(
        runs.iter()
            .flat_map(|run| &run.chunks)
            .all(|chunk| chunk.outcome == "not_handled" || chunk.outcome == "failed")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_model_ollama_does_not_list_ends_the_task_at_once() {
    let rig = Rig::start_listing(
        vec![plan("All.", &[("Anything?", &[])])],
        nothing_relevant(),
        "other-model:latest",
    )
    .await;
    let error = rig
        .fan_out()
        .run(&task("What happened?", 1))
        .await
        .unwrap_err();
    assert_eq!(error.kind, FailureKind::Local);
    assert_eq!(rig.local_chats(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn kept_results_past_sixteen_are_held_back_and_copied_quotes_discarded() {
    let every_chunk: LocalScript = Arc::new(|chunk: &str, question: &str| {
        let line = chunk.lines().next().unwrap_or_default().to_string();
        let finding = if question.contains("copy") {
            line.clone()
        } else {
            "This chunk mentions a shift.".to_string()
        };
        Local::Answer(json!({"relevant": true, "finding": finding, "quotes": [line]}))
    });
    let mut long_task = task("What happened?", 1);
    long_task.documents[0].text = crate::support::shift_log().repeat(12);
    let rig = Rig::start(
        vec![
            plan("All.", &[("Anything?", &[]), ("Please copy.", &["Day"])]),
            crate::support::answer("x", &["C1"]),
        ],
        every_chunk,
    )
    .await;
    rig.fan_out().run(&long_task).await.unwrap();
    let body: serde_json::Value = serde_json::from_str(&rig.cloud_bodies()[1]).unwrap();
    let report: serde_json::Value =
        serde_json::from_str(body["messages"][2]["content"].as_str().unwrap()).unwrap();
    let first = &report["jobs"][0];
    let sent = first["results"].as_array().unwrap().len();
    assert!(sent <= 16, "{sent}");
    if first["chunks"].as_u64().unwrap() > 16 {
        assert!(first["held_back"].as_u64().unwrap() > 0);
    }
    assert!(
        report["jobs"][1]["discarded"]["copied_quote"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        rig.job_runs()
            .iter()
            .flat_map(|run| &run.chunks)
            .any(|chunk| chunk.outcome == "held_back")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_time_limit_during_local_runs_records_them() {
    let stall: LocalScript = Arc::new(|_chunk: &str, _question: &str| Local::Stall);
    let rig = Rig::start(vec![plan("All.", &[("Anything?", &["seal"])])], stall).await;
    let mut slow = task("What happened?", 1);
    slow.limits.seconds = 2;
    let error = rig.fan_out().run(&slow).await.unwrap_err();
    assert_eq!(error.kind, FailureKind::Timeout);
    let runs = rig.job_runs();
    assert!(!runs.is_empty());
    assert!(
        runs[0]
            .chunks
            .iter()
            .all(|chunk| chunk.error_kind.as_deref() == Some("timeout"))
    );
    assert_eq!(rig.task_end().error_kind.as_deref(), Some("timeout"));
}

#[tokio::test(flavor = "multi_thread")]
async fn errors_and_debug_output_hold_no_material() {
    let rig = Rig::start(
        vec![plan("Probe.", &[("Who?", &["Zeynep Kaya"])])],
        quoting("seal", "x"),
    )
    .await;
    let the_task = task("Did Zeynep Kaya replace a seal?", 1);
    let error = rig.fan_out().run(&the_task).await.unwrap_err();
    for text in [
        format!("{error}"),
        format!("{error:?}"),
        format!("{the_task:?}"),
    ] {
        for material in ["Zeynep", "inner seal", "shift-log", "ran normally"] {
            assert!(!text.contains(material), "{material} in {text}");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_budget_wider_than_the_endpoints_is_refused_before_anything_is_sent() {
    let rig = crate::support::Rig::start(
        Vec::new(),
        crate::support::quoting("inner seal", "the seal was replaced"),
    )
    .await;
    let mut wider = rig.cloud.config().task_limits();
    wider.requests += 1;
    let fan_out = snet_fanout::FanOut {
        budget: wider,
        ..rig.fan_out()
    };
    let error = fan_out
        .run(&crate::support::task("Which seal was replaced?", 2))
        .await
        .unwrap_err();
    assert_eq!(error.kind, snet_fanout::FailureKind::Input);
    assert_eq!(rig.cloud_server.observations.connections(), 0);
    assert_eq!(rig.kinds(), ["task_refused"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_narrower_budget_stops_the_task_at_its_request_limit() {
    let rig = crate::support::Rig::start(
        vec![
            crate::support::plan(
                "Find the seal work.",
                &[("Which seal was replaced?", &["seal"])],
            ),
            crate::support::answer("The inner seal of pump 3.", &["C1"]),
        ],
        crate::support::quoting("inner seal", "the inner seal was replaced"),
    )
    .await;
    let mut narrower = rig.cloud.config().task_limits();
    narrower.requests = 1;
    let fan_out = snet_fanout::FanOut {
        budget: narrower,
        ..rig.fan_out()
    };
    let error = fan_out
        .run(&crate::support::task("Which seal was replaced?", 2))
        .await
        .unwrap_err();
    assert_eq!(error.kind, snet_fanout::FailureKind::Cloud);
    assert_eq!(rig.cloud_server.observations.requests().len(), 1);
    assert_eq!(rig.task_end().charge.requests, "1");
}
