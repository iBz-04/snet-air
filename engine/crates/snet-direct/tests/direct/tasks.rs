use serde_json::json;
use snet_protocol::FailureKind;

use crate::support::{Cloud, Rig, task};

const EMAIL: &str = "ayse.demir@example.com";

#[tokio::test]
async fn a_question_is_pseudonymised_sent_once_and_restored() {
    let rig = Rig::start(vec![Cloud::Reply(
        json!({"answer": "Write to ⟦EMAIL_1⟧ and attach the form."}),
    )])
    .await;
    let question = format!("How should I ask {EMAIL} for the refund?");
    let answer = rig.direct().run(&task(&question, &[], 30)).await.unwrap();
    assert_eq!(
        answer.text.as_str(),
        format!("Write to {EMAIL} and attach the form.")
    );
    assert_eq!(answer.charge.requests, 1);
    let bodies = rig.cloud_bodies();
    assert_eq!(bodies.len(), 1);
    assert!(!bodies[0].contains(EMAIL));
    assert!(bodies[0].contains("⟦EMAIL_1⟧"));
    assert_eq!(
        rig.kinds(),
        vec!["direct_started", "outbound", "outcome", "task_ended"]
    );
    let records = rig.records();
    assert_eq!(records[0]["question"], question);
    assert_eq!(records[3]["outcome"], "answer");
}

#[tokio::test]
async fn earlier_exchanges_go_with_the_question_pseudonymised() {
    let rig = Rig::start(vec![Cloud::Reply(
        json!({"answer": "Yes, the same address."}),
    )])
    .await;
    let earlier = [("Who handles refunds?", "Write to ayse.demir@example.com.")];
    rig.direct()
        .run(&task("Is that still the right address?", &earlier, 30))
        .await
        .unwrap();
    let body = &rig.cloud_bodies()[0];
    assert!(body.contains("Who handles refunds?"));
    assert!(body.contains("⟦EMAIL_1⟧"));
    assert!(!body.contains(EMAIL));
    assert_eq!(rig.records()[0]["earlier_exchanges"], 1);
}

#[tokio::test]
async fn bad_input_is_refused_before_anything_is_sent() {
    let rig = Rig::start(Vec::new()).await;
    let too_many: Vec<(&str, &str)> = (0..9).map(|_| ("q?", "a.")).collect();
    for (bad, field) in [
        (task(" ", &[], 30), "question"),
        (task("Why?", &too_many, 30), "earlier"),
        (task("Why?", &[("q?", "")], 30), "earlier[0].answer"),
        (task("Why?", &[], 0), "seconds"),
    ] {
        let error = rig.direct().run(&bad).await.unwrap_err();
        assert_eq!(error.kind, FailureKind::Input);
        assert!(error.detail.contains(field), "{}", error.detail);
    }
    assert!(rig.cloud_bodies().is_empty());
    assert!(rig.kinds().iter().all(|kind| kind == "task_refused"));
}

#[tokio::test]
async fn a_reply_off_the_schema_or_with_an_unknown_pseudonym_fails_the_task() {
    for reply in [
        json!({"answer": "   "}),
        json!({"answer": "Write to ⟦EMAIL_7⟧."}),
    ] {
        let rig = Rig::start(vec![Cloud::Reply(reply)]).await;
        let error = rig
            .direct()
            .run(&task("Who handles refunds?", &[], 30))
            .await
            .unwrap_err();
        assert_eq!(error.kind, FailureKind::InvalidCloudReply);
        let end = rig.records().pop().unwrap();
        assert_eq!(end["kind"], "task_ended");
        assert_eq!(end["outcome"], "failed");
    }
}

#[tokio::test]
async fn a_stalled_cloud_ends_on_the_time_limit() {
    let rig = Rig::start(vec![Cloud::Stall]).await;
    let error = rig
        .direct()
        .run(&task("Who handles refunds?", &[], 1))
        .await
        .unwrap_err();
    assert!(
        matches!(error.kind, FailureKind::Timeout | FailureKind::Cloud),
        "{error:?}"
    );
    assert_eq!(rig.records().pop().unwrap()["outcome"], "failed");
}
