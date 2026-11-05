use std::sync::Arc;

use serde_json::{Value, json};
use snet_fanout::FanOutOutcome;

use crate::support::{LEAK_LINE, Local, Rig, SEAL_LINE, answer, plan, task};

#[tokio::test(flavor = "multi_thread")]
async fn a_plan_runs_on_matching_chunks_and_only_checked_findings_leave() {
    let script = Arc::new(|chunk: &str, question: &str| {
        let (needle, finding) = if question.contains("seal") {
            ("seal of pump", "The inner seal of pump 3 was replaced.")
        } else {
            ("leak", "Ahmet Yılmaz reported a valve leak.")
        };
        match chunk.lines().find(|line| line.contains(needle)) {
            Some(line) => {
                Local::Answer(json!({"relevant": true, "finding": finding, "quotes": [line]}))
            }
            None => Local::Answer(json!({"relevant": false, "finding": "", "quotes": []})),
        }
    });
    let rig = Rig::start(
        vec![
            plan(
                "Find the seal and the leak.",
                &[
                    ("Which seal was replaced?", &["seal"]),
                    ("Who reported a leak?", &[]),
                ],
            ),
            answer(
                "The inner seal of pump 3 was replaced after ⟦PERSON_1⟧ reported a leak.",
                &["C1", "C2"],
            ),
        ],
        script,
    )
    .await;
    let result = rig
        .fan_out()
        .run(&task("What happened to pump 3?", 1))
        .await
        .unwrap();
    let FanOutOutcome::Answer { text, citations } = &result.outcome else {
        panic!("{:?}", result.outcome);
    };
    assert_eq!(
        text.as_str(),
        "The inner seal of pump 3 was replaced after Ahmet Yılmaz reported a leak."
    );
    let texts: Vec<&str> = citations
        .iter()
        .map(|passage| passage.text.as_str())
        .collect();
    assert!(
        texts.contains(&SEAL_LINE) && texts.contains(&LEAK_LINE),
        "{texts:?}"
    );
    for passage in citations {
        let document = if passage.document == "shift-log.txt" {
            crate::support::shift_log()
        } else {
            crate::support::maintenance()
        };
        assert_eq!(&document[passage.range.clone()], passage.text);
    }

    let bodies = rig.cloud_bodies();
    assert_eq!(bodies.len(), 2);
    for body in &bodies {
        for material in [
            "shift-log",
            "maintenance.txt",
            SEAL_LINE,
            LEAK_LINE,
            "Ahmet",
            "532 123",
            "ran normally",
        ] {
            assert!(!body.contains(material), "{material} in {body}");
        }
    }
    let first: Value = serde_json::from_str(&bodies[0]).unwrap();
    let message: Value =
        serde_json::from_str(first["messages"][0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(message["plan_rounds"], 1);
    assert_eq!(message["local_requests_left"], 200);
    let chunk_counts = message["chunks"].as_array().unwrap().clone();
    let total: u64 = chunk_counts
        .iter()
        .map(|count| count.as_u64().unwrap())
        .sum();
    assert!(total >= 3, "{chunk_counts:?}");
    let schema_steps = |body: &Value| {
        body["output_config"]["format"]["schema"]["properties"]["step"]["enum"].clone()
    };
    assert_eq!(schema_steps(&first), json!(["plan", "not_found"]));
    let second: Value = serde_json::from_str(&bodies[1]).unwrap();
    assert_eq!(schema_steps(&second), json!(["answer", "not_found"]));
    let report: Value =
        serde_json::from_str(second["messages"][2]["content"].as_str().unwrap()).unwrap();
    let jobs = report["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[1]["chunks"].as_u64().unwrap(), total);
    assert_eq!(
        jobs[0]["results"][0]["finding"],
        "The inner seal of pump 3 was replaced."
    );
    assert_eq!(
        jobs[1]["results"][0]["finding"],
        "⟦PERSON_1⟧ reported a valve leak."
    );
    assert_eq!(jobs[1]["results"][0]["chunk"].as_array().unwrap().len(), 2);
    for reason in [
        "not_relevant",
        "too_long",
        "unverified",
        "copied_quote",
        "bracket",
        "spelled_value",
    ] {
        assert!(jobs[1]["discarded"][reason].is_u64(), "{reason}");
    }
    assert_eq!(
        rig.local_chats() as u64,
        jobs[0]["chunks"].as_u64().unwrap() + total
    );
    assert_eq!(
        report["local_requests_left"].as_u64().unwrap(),
        200 - rig.local_chats() as u64
    );

    let kinds = rig.kinds();
    assert_eq!(kinds.iter().filter(|kind| *kind == "job_run").count(), 2);
    assert!(
        kinds.contains(&"fan_out_started".to_string())
            && kinds.last().map(String::as_str) == Some("task_ended")
    );
    let runs = rig.job_runs();
    assert!(runs[1].chunks.iter().any(|chunk| chunk.outcome == "sent"));
    for line in rig.records() {
        if line["kind"] == "outbound" {
            let body = line["body"].as_str().unwrap();
            assert!(!body.contains("Ahmet"), "{body}");
            assert!(!body.contains("532 123"), "{body}");
        }
    }
    assert_eq!(result.cloud_requests, 2);
    assert_eq!(rig.task_end().outcome, "answer");
}

#[tokio::test(flavor = "multi_thread")]
async fn terms_select_chunks_on_word_boundaries_and_a_token_term_selects_its_person() {
    let rig = Rig::start(
        vec![
            plan(
                "Probe.",
                &[("Anything?", &["hmet"]), ("Leak?", &["⟦PERSON_1⟧"])],
            ),
            not_found(),
        ],
        crate::support::quoting("leak", "A leak was reported."),
    )
    .await;
    let mut the_task = task("Did Ahmet Yılmaz report anything?", 1);
    the_task.limits.local_requests = 200;
    rig.fan_out().run(&the_task).await.unwrap();
    let runs = rig.job_runs();
    assert_eq!(
        runs.iter()
            .find(|run| run.job == 1)
            .map(|run| run.chunks.len()),
        Some(0)
    );
    assert_eq!(
        runs.iter()
            .find(|run| run.job == 2)
            .map(|run| run.chunks.len()),
        Some(1)
    );
    assert_eq!(runs[1].terms, vec!["Ahmet Yılmaz".to_string()]);
}

fn not_found() -> crate::support::Cloud {
    crate::support::Cloud::Step(
        json!({"step": "not_found", "text": "Not in the material.", "jobs": [], "citations": []}),
    )
}
