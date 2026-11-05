use serde_json::Value;
use sha2::{Digest, Sha256};
use snet_converse::{CLOUD_PROMPT, ConversationOutcome, LOCAL_PROMPT};

use crate::support::{Rig, SEAL_QUOTE, answer, ask, local, task};

fn schema_steps(body: &str) -> Vec<String> {
    let body: Value = serde_json::from_str(body).unwrap();
    body["output_config"]["format"]["schema"]["properties"]["step"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step.as_str().unwrap().to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_two_round_task_answers_with_checked_citations_and_sends_no_material() {
    let rig = Rig::start(
        vec![
            ask("Who replaced the seal of pump 3, and which seal?"),
            answer("⟦PERSON_1⟧ replaced the inner seal of pump 3.", &["C1"]),
        ],
        vec![local(
            "Zeynep Kaya replaced the inner seal.",
            &[("D1", SEAL_QUOTE)],
        )],
    )
    .await;
    let result = rig
        .conversation()
        .run(&task("Which seal of pump 3 was replaced, and by whom?", 2))
        .await
        .unwrap();
    let ConversationOutcome::Answer { text, citations } = &result.outcome else {
        panic!("{:?}", result.outcome);
    };
    assert_eq!(
        text.as_str(),
        "Zeynep Kaya replaced the inner seal of pump 3."
    );
    assert_eq!(citations.len(), 1);
    assert_eq!(citations[0].citation, "C1");
    assert_eq!(citations[0].document, "shift-log.txt");
    assert_eq!(citations[0].text, SEAL_QUOTE);
    assert_eq!(
        &crate::support::SHIFT_LOG[citations[0].range.clone()],
        SEAL_QUOTE
    );
    assert_eq!(result.rounds, 2);
    assert_eq!(result.local_tokens, 340);

    let bodies = rig.cloud_bodies();
    assert_eq!(bodies.len(), 2);
    for body in &bodies {
        for material in [
            "Zeynep",
            "Ahmet",
            "shift-log",
            SEAL_QUOTE,
            "532 123",
            "09:40",
        ] {
            assert!(!body.contains(material), "{material} in {body}");
        }
    }
    assert_eq!(schema_steps(&bodies[0]), ["ask", "not_found"]);
    assert_eq!(schema_steps(&bodies[1]), ["answer", "not_found"]);
    let second: Value = serde_json::from_str(&bodies[1]).unwrap();
    let report: Value =
        serde_json::from_str(second["messages"][2]["content"].as_str().unwrap()).unwrap();
    assert_eq!(report["answer"], "⟦PERSON_1⟧ replaced the inner seal.");
    assert_eq!(report["citations"], serde_json::json!(["C1"]));
    assert_eq!(report["unverified_quotes"], 0);
    assert_eq!(report["final_round"], true);
    assert_eq!(second["messages"][1]["role"], "assistant");

    assert_eq!(
        rig.kinds(),
        [
            "task_started",
            "outbound",
            "outcome",
            "local_exchange",
            "outbound",
            "outcome",
            "task_ended"
        ]
    );
    for line in rig.records() {
        if line["kind"] == "outbound" {
            let body = line["body"].as_str().unwrap();
            assert!(!body.contains("Zeynep"), "{body}");
            assert!(!body.contains("Ahmet"), "{body}");
        }
    }
    assert!(
        rig.cloud_bodies()
            .iter()
            .any(|body| body.contains("⟦PERSON_1⟧"))
    );
    let started = rig.line("task_started");
    let hex = |text: &str| -> String {
        Sha256::digest(text.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    };
    assert_eq!(
        started["cloud_prompt_sha256"],
        hex(&std::fs::read_to_string("prompts/cloud.txt").unwrap())
    );
    assert_eq!(
        started["local_prompt_sha256"],
        hex(&std::fs::read_to_string("prompts/local.txt").unwrap())
    );
    assert_eq!(
        CLOUD_PROMPT,
        std::fs::read_to_string("prompts/cloud.txt").unwrap()
    );
    assert_eq!(
        LOCAL_PROMPT,
        std::fs::read_to_string("prompts/local.txt").unwrap()
    );
    let ended = rig.task_end();
    assert_eq!(ended["outcome"], "answer");
    assert_eq!(
        ended["citations"][0]["start"],
        citations[0].range.start as u64
    );
    assert_eq!(result.task_end_line, rig.records().len() as u64);
}

#[tokio::test(flavor = "multi_thread")]
async fn quotes_are_verified_exactly_and_repeated_quotes_keep_their_citation() {
    let rig = Rig::start(
        vec![
            ask("Who replaced the seal?"),
            ask("And the seal type?"),
            answer("The inner seal.", &["C1"]),
        ],
        vec![
            local(
                "The seal was replaced.",
                &[
                    ("D1", SEAL_QUOTE),
                    ("D1", "Zeynep Kaya changed the seal"),
                    ("D1", "pump 3"),
                ],
            ),
            local("The inner one.", &[("D1", SEAL_QUOTE)]),
        ],
    )
    .await;
    rig.conversation()
        .run(&task("Which seal was replaced?", 3))
        .await
        .unwrap();
    let bodies = rig.cloud_bodies();
    let third: Value = serde_json::from_str(&bodies[2]).unwrap();
    let first_report: Value =
        serde_json::from_str(third["messages"][2]["content"].as_str().unwrap()).unwrap();
    let second_report: Value =
        serde_json::from_str(third["messages"][4]["content"].as_str().unwrap()).unwrap();
    assert_eq!(first_report["citations"], serde_json::json!(["C1"]));
    assert_eq!(first_report["unverified_quotes"], 2);
    assert_eq!(first_report["final_round"], false);
    assert_eq!(second_report["citations"], serde_json::json!(["C1"]));
    assert_eq!(second_report["final_round"], true);
    assert_eq!(schema_steps(&bodies[0]), ["ask", "not_found"]);
    assert_eq!(schema_steps(&bodies[1]), ["ask", "answer", "not_found"]);
    assert_eq!(schema_steps(&bodies[2]), ["answer", "not_found"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn not_found_ends_the_task_without_citations() {
    let rig = Rig::start(
        vec![crate::support::Cloud::Step(serde_json::json!({"step": "not_found", "text": "The log does not say.", "citations": []}))],
        Vec::new(),
    )
    .await;
    let result = rig
        .conversation()
        .run(&task("Who repaired pump 9?", 2))
        .await
        .unwrap();
    assert!(
        matches!(&result.outcome, ConversationOutcome::NotFound { text } if text.as_str() == "The log does not say.")
    );
    assert_eq!(rig.task_end()["outcome"], "not_found");
    assert_eq!(rig.local_server.observations.connections(), 0);
}
