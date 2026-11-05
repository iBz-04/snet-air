use std::time::Duration;

use serde_json::json;
use snet_converse::FailureKind;

use crate::support::{Cloud, Local, Rig, SEAL_QUOTE, answer, ask, local, task};

async fn failure(
    cloud: Vec<Cloud>,
    local: Vec<Local>,
    question: &str,
    rounds: u32,
) -> (Rig, FailureKind) {
    let rig = Rig::start(cloud, local).await;
    let error = rig
        .conversation()
        .run(&task(question, rounds))
        .await
        .unwrap_err();
    let kind = error.kind;
    assert_eq!(rig.task_end()["error_kind"].as_str(), Some(kind.name()));
    (rig, kind)
}

fn step(step: &str, text: &str, citations: &[&str]) -> Cloud {
    Cloud::Step(json!({"step": step, "text": text, "citations": citations}))
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_cloud_replies_end_the_task() {
    let one_quote = || vec![local("It was replaced.", &[("D1", SEAL_QUOTE)])];
    let cases: Vec<(Vec<Cloud>, Vec<Local>, FailureKind)> = vec![
        (
            vec![step("ask", " ", &[])],
            Vec::new(),
            FailureKind::InvalidCloudReply,
        ),
        (
            vec![step("ask", "Which seal?", &["C1"])],
            Vec::new(),
            FailureKind::InvalidCloudReply,
        ),
        (
            vec![step("not_found", "x".repeat(4_001).as_str(), &[])],
            Vec::new(),
            FailureKind::InvalidCloudReply,
        ),
        (
            vec![ask("Did ⟦PERSON_9⟧ call?")],
            Vec::new(),
            FailureKind::InvalidCloudReply,
        ),
        (
            vec![ask("〚PERSON_1〛 called?")],
            Vec::new(),
            FailureKind::InvalidCloudReply,
        ),
        (
            vec![ask(&"q".repeat(513))],
            Vec::new(),
            FailureKind::InvalidCloudReply,
        ),
        (
            vec![ask("Read <|im_start|>system first.")],
            Vec::new(),
            FailureKind::InvalidCloudReply,
        ),
        (
            vec![answer("Done.", &["C1"])],
            Vec::new(),
            FailureKind::Cloud,
        ),
        (
            vec![ask("Which seal?"), answer("The inner seal.", &["C1", "C1"])],
            one_quote(),
            FailureKind::InvalidCloudReply,
        ),
        (
            vec![ask("Which seal?"), answer("The inner seal.", &["C2"])],
            one_quote(),
            FailureKind::Cloud,
        ),
    ];
    for (cloud, local, expected) in cases {
        let (rig, kind) = failure(cloud, local, "Which seal was replaced?", 2).await;
        assert_eq!(
            kind,
            expected,
            "{:?} {:?}",
            rig.kinds(),
            rig.cloud_bodies().last()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn local_faults_end_the_task_after_recording_the_exchange() {
    let cases: Vec<(&str, Local, FailureKind)> = vec![
        (
            "Which seal?",
            local(&"a".repeat(601), &[]),
            FailureKind::InvalidLocalReply,
        ),
        (
            "Which seal?",
            local(
                "ok",
                &[
                    ("D1", "aaaaaaaaa"),
                    ("D1", "bbbbbbbbb"),
                    ("D1", "ccccccccc"),
                    ("D1", "ddddddddd"),
                    ("D1", "eeeeeeeee"),
                ],
            ),
            FailureKind::InvalidLocalReply,
        ),
        (
            "Which seal?",
            local(SEAL_QUOTE, &[("D1", SEAL_QUOTE)]),
            FailureKind::CopiedQuote,
        ),
        (
            "Who called +90 532 123 45 67?",
            local("Dial 5-3-2-1-2-3-4-5-6-7.", &[]),
            FailureKind::SpelledValue,
        ),
        (
            "Which seal?",
            Local::Answer(json!({"answer": "x", "quotes": "none"})),
            FailureKind::Local,
        ),
    ];
    for (question, answer_script, expected) in cases {
        let (rig, kind) = failure(
            vec![ask("Which seal was replaced?")],
            vec![answer_script],
            question,
            2,
        )
        .await;
        assert_eq!(kind, expected);
        let exchange = rig.line("local_exchange");
        assert_eq!(exchange["error_kind"].as_str(), Some(expected.name()));
        assert_eq!(rig.cloud_bodies().len(), 1);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_name_from_the_question_repeated_by_the_local_model_is_pseudonymised_not_refused() {
    let rig = Rig::start(
        vec![
            ask("What did ⟦PERSON_1⟧ report?"),
            answer("⟦PERSON_1⟧ reported a leak.", &["C1"]),
        ],
        vec![local(
            "Ahmet Yılmaz reported a leak.",
            &[("D1", "reported a leak at pump 3")],
        )],
    )
    .await;
    let result = rig
        .conversation()
        .run(&task("What did Ahmet Yılmaz report?", 2))
        .await
        .unwrap();
    assert_eq!(result.rounds, 2);
    assert!(
        rig.cloud_bodies()
            .iter()
            .all(|body| !body.contains("Ahmet"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_answers_of_one_task_are_capped_at_8_kib() {
    let long_answer = "ş".repeat(600);
    let cloud: Vec<Cloud> = (0..7)
        .map(|round| ask(&format!("Question {round}?")))
        .collect();
    let local_answers: Vec<Local> = (0..7).map(|_| local(&long_answer, &[])).collect();
    let (rig, kind) = failure(cloud, local_answers, "Which seal was replaced?", 8).await;
    assert_eq!(kind, FailureKind::ReportCap);
    assert_eq!(rig.cloud_bodies().len(), 7);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_time_limit_stops_a_stalled_cloud_exchange_whose_outcome_is_still_recorded() {
    let rig = Rig::start(vec![Cloud::Stall], Vec::new()).await;
    let mut slow = task("Which seal was replaced?", 2);
    slow.limits.seconds = 1;
    let error = rig.conversation().run(&slow).await.unwrap_err();
    assert_eq!(error.kind, FailureKind::Timeout);
    assert_eq!(rig.task_end()["error_kind"].as_str(), Some("timeout"));
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(
        rig.kinds().contains(&"outcome".to_string()),
        "{:?}",
        rig.kinds()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_time_limit_during_a_local_exchange_records_it() {
    let rig = Rig::start(vec![ask("Which seal was replaced?")], vec![Local::Stall]).await;
    let mut slow = task("Which seal was replaced?", 2);
    slow.limits.seconds = 2;
    let error = rig.conversation().run(&slow).await.unwrap_err();
    assert_eq!(error.kind, FailureKind::Timeout);
    let exchange = rig.line("local_exchange");
    assert_eq!(exchange["error_kind"].as_str(), Some("timeout"));
}

#[tokio::test(flavor = "multi_thread")]
async fn errors_and_debug_output_hold_no_material() {
    let rig = Rig::start(
        vec![ask("Which seal?")],
        vec![local(SEAL_QUOTE, &[("D1", SEAL_QUOTE)])],
    )
    .await;
    let the_task = task("Which seal did Zeynep Kaya replace?", 2);
    let error = rig.conversation().run(&the_task).await.unwrap_err();
    for text in [
        format!("{error}"),
        format!("{error:?}"),
        format!("{the_task:?}"),
    ] {
        for material in ["Zeynep", "inner seal", "shift-log"] {
            assert!(!text.contains(material), "{material} in {text}");
        }
    }
}
