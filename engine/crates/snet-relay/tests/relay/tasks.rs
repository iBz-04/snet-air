use serde_json::json;
use snet_protocol::FailureKind;

use crate::support::{Cloud, Local, Rig, checked, screened, task};

const PERSON: &str = "Ayşe Demir";
const COMPANY: &str = "Kaya Lojistik";
const EMAIL: &str = "ayse.demir@example.com";

#[tokio::test]
async fn the_local_model_screens_and_only_its_question_reaches_the_cloud() {
    let question = format!(
        "My manager {PERSON} at {COMPANY} wants me to sign a 3-year exclusivity clause. Mail her at {EMAIL}? Is the clause normal?"
    );
    let rig = Rig::start(
        vec![Cloud::Reply(
            json!({"answer": "Ask ⟦ORGANISATION_1⟧ for a shorter term; three years is long."}),
        )],
        vec![
            screened(
                &format!("Is a 3-year exclusivity clause from {COMPANY} normal for an employee?"),
                &[("PERSON", PERSON), ("ORGANISATION", COMPANY)],
            ),
            checked(&[]),
        ],
    )
    .await;
    let answer = rig.relay().run(&task(&question, &[], 30)).await.unwrap();

    assert_eq!(
        answer.text.as_str(),
        format!("Ask {COMPANY} for a shorter term; three years is long.")
    );
    assert_eq!(
        answer.sent_question,
        "Is a 3-year exclusivity clause from ⟦ORGANISATION_1⟧ normal for an employee?"
    );
    assert_eq!(answer.local_tokens, 680);
    let local = rig.local_bodies();
    assert_eq!(local.len(), 2);
    assert!(
        local[0].contains(EMAIL),
        "the local model reads the question"
    );
    let cloud = rig.cloud_bodies();
    assert_eq!(cloud.len(), 1);
    for private in [PERSON, COMPANY, EMAIL, "manager"] {
        assert!(!cloud[0].contains(private), "{private} reached the cloud");
    }
    assert_eq!(
        rig.kinds(),
        vec![
            "relay_started",
            "local_exchange",
            "local_exchange",
            "outbound",
            "outcome",
            "task_ended"
        ]
    );
    assert_eq!(rig.records()[5]["local_tokens"], 680);
}

#[tokio::test]
async fn earlier_exchanges_go_only_to_the_local_model() {
    let rig = Rig::start(
        vec![Cloud::Reply(json!({"answer": "About 450 000 people."}))],
        vec![
            screened("What is the population of Canberra?", &[]),
            checked(&[]),
        ],
    )
    .await;
    let earlier = [("What is the capital of Australia?", "Canberra.")];
    rig.relay()
        .run(&task("And its population?", &earlier, 30))
        .await
        .unwrap();
    assert!(rig.local_bodies()[0].contains("What is the capital of Australia?"));
    let cloud = &rig.cloud_bodies()[0];
    assert!(cloud.contains("What is the population of Canberra?"));
    assert!(!cloud.contains("capital of Australia"));
    assert_eq!(rig.records()[0]["earlier_exchanges"], 1);
}

#[tokio::test]
async fn built_in_detectors_still_run_on_what_the_local_model_wrote() {
    let rig = Rig::start(
        vec![Cloud::Reply(json!({"answer": "Write to ⟦EMAIL_1⟧."}))],
        vec![
            screened(&format!("How do I ask {EMAIL} for a refund?"), &[]),
            checked(&[]),
        ],
    )
    .await;
    let answer = rig
        .relay()
        .run(&task(&format!("Refund from {EMAIL}?"), &[], 30))
        .await
        .unwrap();
    assert!(!rig.cloud_bodies()[0].contains(EMAIL));
    assert_eq!(answer.text.as_str(), format!("Write to {EMAIL}."));
}

#[tokio::test]
async fn a_bad_local_reply_fails_before_anything_is_sent() {
    for local in [
        Local::Screened(json!({"cloud_question": "", "private_terms": []})),
        Local::Screened(
            json!({"cloud_question": "Why?", "private_terms": [{"category": "NAME", "text": "Jo"}]}),
        ),
    ] {
        let rig = Rig::start(Vec::new(), vec![local]).await;
        let error = rig
            .relay()
            .run(&task("Why is the sky blue?", &[], 30))
            .await
            .unwrap_err();
        assert!(
            matches!(
                error.kind,
                FailureKind::InvalidLocalReply | FailureKind::Local
            ),
            "{error:?}"
        );
        assert!(rig.cloud_bodies().is_empty());
        assert_eq!(rig.records().pop().unwrap()["outcome"], "failed");
    }
}

#[tokio::test]
async fn bad_input_is_refused_before_anything_runs() {
    let rig = Rig::start(Vec::new(), Vec::new()).await;
    let too_many: Vec<(&str, &str)> = (0..9).map(|_| ("q?", "a.")).collect();
    for (bad, field) in [
        (task(" ", &[], 30), "question"),
        (task("Why?", &too_many, 30), "earlier"),
        (task("Why?", &[("q?", "")], 30), "earlier[0].answer"),
        (task("Why?", &[], 0), "seconds"),
    ] {
        let error = rig.relay().run(&bad).await.unwrap_err();
        assert_eq!(error.kind, FailureKind::Input);
        assert!(error.detail.contains(field), "{}", error.detail);
    }
    assert!(rig.local_bodies().is_empty());
    assert!(rig.cloud_bodies().is_empty());
    assert!(rig.kinds().iter().all(|kind| kind == "task_refused"));
}

#[tokio::test]
async fn a_cloud_reply_with_an_unknown_pseudonym_fails_the_task() {
    let rig = Rig::start(
        vec![Cloud::Reply(json!({"answer": "Write to ⟦EMAIL_7⟧."}))],
        vec![screened("Who handles refunds?", &[]), checked(&[])],
    )
    .await;
    let error = rig
        .relay()
        .run(&task("Who handles refunds?", &[], 30))
        .await
        .unwrap_err();
    assert_eq!(error.kind, FailureKind::InvalidCloudReply);
    assert_eq!(rig.records().pop().unwrap()["outcome"], "failed");
}

#[tokio::test]
async fn a_stalled_local_model_ends_on_the_time_limit_with_nothing_sent() {
    let rig = Rig::start(Vec::new(), vec![Local::Stall]).await;
    let error = rig
        .relay()
        .run(&task("Who handles refunds?", &[], 1))
        .await
        .unwrap_err();
    assert!(
        matches!(error.kind, FailureKind::Timeout | FailureKind::Local),
        "{error:?}"
    );
    assert!(rig.cloud_bodies().is_empty());
}

#[tokio::test]
async fn a_stalled_cloud_ends_on_the_time_limit_after_the_local_step() {
    let rig = Rig::start(
        vec![Cloud::Stall],
        vec![screened("Who handles refunds?", &[]), checked(&[])],
    )
    .await;
    let error = rig
        .relay()
        .run(&task("Who handles refunds?", &[], 2))
        .await
        .unwrap_err();
    assert!(
        matches!(error.kind, FailureKind::Timeout | FailureKind::Cloud),
        "{error:?}"
    );
    assert_eq!(rig.local_bodies().len(), 2);
    assert_eq!(rig.records().pop().unwrap()["outcome"], "failed");
}

#[tokio::test]
async fn the_check_pass_catches_a_name_the_first_pass_kept() {
    let rig = Rig::start(
        vec![Cloud::Reply(
            json!({"answer": "190 is high; ask ⟦PERSON_1⟧ about treatment."}),
        )],
        vec![
            screened(
                "My doctor Dr. Okan Yilmaz says my LDL is 190. Should I worry?",
                &[],
            ),
            checked(&[("PERSON", "Okan Yilmaz")]),
        ],
    )
    .await;
    let answer = rig
        .relay()
        .run(&task(
            "My doctor Dr. Okan Yilmaz says my LDL is 190. Should I worry?",
            &[],
            30,
        ))
        .await
        .unwrap();
    let cloud = &rig.cloud_bodies()[0];
    assert!(
        !cloud.contains("Okan") && !cloud.contains("Yilmaz"),
        "{cloud}"
    );
    assert_eq!(
        answer.sent_question,
        "My doctor Dr. ⟦PERSON_1⟧ says my LDL is 190. Should I worry?"
    );
    assert_eq!(
        answer.text.as_str(),
        "190 is high; ask Okan Yilmaz about treatment."
    );
    assert!(rig.local_bodies()[1].contains("Okan Yilmaz"));
}
