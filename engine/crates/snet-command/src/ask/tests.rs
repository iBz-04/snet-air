use std::sync::Arc;

use serde_json::{Value, json};
use snet_testkit::certificates::{CA_FILE, TestPki, Validity};
use snet_testkit::http::{http_response, json_response};
use snet_testkit::loopback::{LoopbackHandler, LoopbackReply, LoopbackServer};
use snet_testkit::tls::{Behaviour, TestServer, TlsVersion};

use super::*;
use crate::test_support::FixedEnvironment;

const LOG: &str = "07:10 Ahmet Yılmaz reported a leak at pump 3, ahmet@example.com.\n09:40 the inner seal of pump 3 was replaced by the night crew.\n";
const QUOTE: &str = "the inner seal of pump 3 was replaced by the night crew";
const DIGEST: &str = "3a256cc5d3b69933c87c12e52ef330494a3f91361020dffc51819c867088b8e0";

fn cloud_reply(object: Value) -> Vec<u8> {
    json_response(
        &json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": "test-model",
            "content": [{"type": "text", "text": object.to_string()}],
            "stop_reason": "end_turn", "usage": {"input_tokens": 100, "output_tokens": 30}
        })
        .to_string(),
    )
}

fn ollama_reply(object: Value) -> Vec<u8> {
    let body = json!({"model": "qwen3:4b", "message": {"role": "assistant", "content": object.to_string()},
        "done": true, "done_reason": "stop", "prompt_eval_count": 300, "eval_count": 40});
    http_response(
        200,
        &[("content-type", "application/json")],
        body.to_string().as_bytes(),
    )
}

fn endpoint_text(port: u16) -> String {
    format!(
        "name = \"test\"\nbase_url = \"https://127.0.0.1:{port}/v1\"\nwire_format = \"anthropic_messages\"\nanthropic_workspace = \"key_scoped\"\neffort = \"model_default\"\nmodel = \"test-model\"\nallow_private_addresses = true\nmax_output_tokens = 8192\ninput_token_allowance = 2048\nrequest_bytes_cap = 1048576\nreply_bytes_cap = 4194304\nconnect_timeout_seconds = 5\ntotal_timeout_seconds = 5\nswitch_starts_on = true\nextra_ca_certificates = [\"{CA_FILE}\"]\nreplace_public_roots = true\n\n[prices]\ncurrency = \"USD\"\ninput_micro_per_million_tokens = 1\noutput_micro_per_million_tokens = 1\n\n[task_limits]\nrequests = 12\nbytes_sent = 2097152\ninput_tokens = 400000\noutput_tokens = 64000\ncost_micro = 1000000\n"
    )
}

/// What the scripted Ollama does with chat requests.
enum Ollama {
    Answers(Value),
    /// One answer per chat request, in order.
    AnswersInTurn(Vec<Value>),
    FailsEveryChat,
    Down,
}

struct Run {
    exit: Result<Exit, CommandError>,
    output: String,
    errors: String,
    cloud_bodies: Vec<String>,
    transcript: Vec<Value>,
}

/// Runs one conversation task against a scripted cloud and Ollama.
async fn run(cloud_script: Vec<Value>, ollama: Ollama) -> Run {
    run_with_files(true, cloud_script, ollama).await
}

/// One `snet ask` against scripted servers, with the shift log as its file
/// or with no file at all.
async fn run_with_files(with_files: bool, cloud_script: Vec<Value>, ollama: Ollama) -> Run {
    let pki = TestPki::new().unwrap();
    let replies: Arc<Vec<Vec<u8>>> = Arc::new(cloud_script.into_iter().map(cloud_reply).collect());
    let behaviour = Behaviour::Handle(Arc::new(move |index, _request| {
        Some(
            replies
                .get(index)
                .cloned()
                .unwrap_or_else(|| http_response(500, &[], b"no more replies")),
        )
    }));
    let identity = pki
        .server_identity(&["127.0.0.1"], Validity::Current)
        .unwrap();
    let cloud = TestServer::start(identity, TlsVersion::Tls13, behaviour)
        .await
        .unwrap();
    let (_ollama_server, ollama_host) = match ollama {
        Ollama::Answers(_) | Ollama::AnswersInTurn(_) | Ollama::FailsEveryChat => {
            let chats = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let handler: LoopbackHandler = Arc::new(move |request| {
                if request.request_line.starts_with("GET /api/tags") {
                    let body = json!({"models": [{"name": "qwen3:4b", "model": "qwen3:4b", "digest": DIGEST, "size": 1}]});
                    return LoopbackReply::Bytes(http_response(
                        200,
                        &[("content-type", "application/json")],
                        body.to_string().as_bytes(),
                    ));
                }
                match &ollama {
                    Ollama::Answers(answer) => LoopbackReply::Bytes(ollama_reply(answer.clone())),
                    Ollama::AnswersInTurn(answers) => {
                        let turn = chats.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        match answers.get(turn) {
                            Some(answer) => LoopbackReply::Bytes(ollama_reply(answer.clone())),
                            None => LoopbackReply::Bytes(http_response(500, &[], b"{}")),
                        }
                    }
                    _ => LoopbackReply::Bytes(http_response(
                        500,
                        &[("content-type", "application/json")],
                        br#"{"error":"model runner stopped"}"#,
                    )),
                }
            });
            let server = LoopbackServer::start(handler).await.unwrap();
            let host = server.address.to_string();
            (Some(server), host)
        }
        Ollama::Down => {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let host = listener.local_addr().unwrap().to_string();
            drop(listener);
            (None, host)
        }
    };

    let home = tempfile::tempdir().unwrap();
    let directory = tempfile::tempdir().unwrap();
    std::fs::copy(pki.ca_path(), directory.path().join(CA_FILE)).unwrap();
    let environment = FixedEnvironment::new(&[
        ("ANTHROPIC_API_KEY", "sk-test-not-a-real-key"),
        ("OLLAMA_HOST", &ollama_host),
        ("SNET_HOME", home.path().to_str().unwrap()),
    ]);
    let arguments = AskArguments {
        question: "Which seal of pump 3 was replaced, and who reported the leak?".to_string(),
        files: if with_files {
            vec!["shift-log.txt".to_string()]
        } else {
            Vec::new()
        },
        ..AskArguments::default()
    };
    let settings = settings::resolve(&arguments, &environment).unwrap();
    let prepared = Prepared {
        settings,
        question: arguments.question.clone(),
        policy: Policy::builtin().unwrap(),
        files: if with_files {
            vec![Document {
                name: "shift-log.txt".to_string(),
                text: LOG.to_string(),
            }]
        } else {
            Vec::new()
        },
        endpoint: EndpointConfig::parse(
            &endpoint_text(cloud.port),
            &snet_testkit::certificates::read_from(directory.path()),
        )
        .unwrap(),
    };
    let mut output = Vec::new();
    let mut errors = Vec::new();
    let local = LocalModel::new(prepared.settings.local.clone());
    let exit = match execute(prepared, &local, &[]).await {
        Ok(finished) => report(finished, &mut output, &mut errors),
        Err(error) => Err(error),
    };
    let transcripts: Vec<_> = match std::fs::read_dir(home.path().join("transcripts")) {
        Ok(entries) => entries.map(|entry| entry.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    };
    assert!(transcripts.len() <= 1);
    Run {
        exit,
        output: String::from_utf8(output).unwrap(),
        errors: String::from_utf8(errors).unwrap(),
        cloud_bodies: cloud
            .observations
            .requests()
            .into_iter()
            .map(|request| String::from_utf8(request.body).unwrap())
            .collect(),
        transcript: transcripts
            .first()
            .map(|path| {
                std::fs::read_to_string(path)
                    .unwrap()
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_answer_is_printed_with_its_passages_and_its_spending() {
    let result = run(
        vec![
            json!({"step": "ask", "text": "Which seal was replaced and who reported the leak?", "citations": []}),
            json!({"step": "answer", "text": "The inner seal; the leak was reported by the person named in the log.", "citations": ["C1"]}),
        ],
        Ollama::Answers(json!({"answer": "The inner seal was replaced.", "quotes": [{"document": "D1", "text": QUOTE}]})),
    )
    .await;
    assert_eq!(result.exit, Ok(Exit::Success));
    assert!(
        result.output.starts_with("The inner seal;"),
        "{}",
        result.output
    );
    assert!(
        result.output.contains("[C1] shift-log.txt, bytes "),
        "{}",
        result.output
    );
    assert!(result.output.contains(&format!("    {QUOTE}")));
    assert!(
        result.errors.contains("rounds 2, cloud requests 2, sent "),
        "{}",
        result.errors
    );
    assert!(
        result.errors.contains("local tokens 340"),
        "{}",
        result.errors
    );
    assert!(result.errors.contains("transcript: "), "{}", result.errors);

    for body in &result.cloud_bodies {
        assert!(!body.contains("ahmet@example.com"), "{body}");
        assert!(!body.contains("sk-test-not-a-real-key"), "{body}");
    }
    let kinds: Vec<&str> = result
        .transcript
        .iter()
        .map(|line| line["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
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
    let sent: Vec<&Value> = result
        .transcript
        .iter()
        .filter(|line| line["kind"] == "outbound")
        .collect();
    assert_eq!(sent[0]["body"], result.cloud_bodies[0].as_str());
    for line in &sent {
        let headers = line["headers"].to_string().to_ascii_lowercase();
        assert!(!headers.contains("x-api-key"), "{headers}");
        assert!(!headers.contains("sk-test"), "{headers}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_task_that_fails_says_why_and_where_its_transcript_is() {
    let result = run(
        vec![json!({"step": "ask", "text": "Which seal was replaced?", "citations": []})],
        Ollama::FailsEveryChat,
    )
    .await;
    assert_eq!(result.exit, Ok(Exit::NoAnswer));
    assert!(result.output.is_empty(), "{}", result.output);
    assert!(
        result.errors.contains("model runner stopped"),
        "{}",
        result.errors
    );
    assert!(result.errors.contains("transcript: "), "{}", result.errors);
    let ended = result.transcript.last().unwrap();
    assert_eq!(ended["kind"], "task_ended");
    assert_eq!(ended["error_kind"], "local");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_ollama_is_reported_before_anything_is_sent() {
    let result = run(
        vec![json!({"step": "ask", "text": "Which seal was replaced?", "citations": []})],
        Ollama::Down,
    )
    .await;
    let error = result.exit.unwrap_err();
    assert_eq!(error.exit, Exit::Configuration);
    assert!(error.message.contains("ollama serve"), "{}", error.message);
    assert!(result.cloud_bodies.is_empty());
    assert!(result.transcript.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_question_without_files_goes_through_both_models() {
    let result = run_with_files(
        false,
        vec![json!({"answer": "Slow start doubles the window each round trip."})],
        Ollama::AnswersInTurn(vec![
            json!({"reply": {"cloud_question": "What is TCP slow start?", "private_terms": []}}),
            json!({"private_terms": []}),
        ]),
    )
    .await;
    assert_eq!(result.exit, Ok(Exit::Success));
    assert_eq!(
        result.output,
        "Slow start doubles the window each round trip.\n"
    );
    assert!(
        result
            .errors
            .contains("sent to the cloud: What is TCP slow start?"),
        "{}",
        result.errors
    );
    assert!(
        result.errors.contains("local tokens 680"),
        "{}",
        result.errors
    );
    assert_eq!(result.cloud_bodies.len(), 1);
    assert_eq!(result.transcript[0]["kind"], "relay_started");
    assert_eq!(result.transcript[1]["kind"], "local_exchange");
    assert_eq!(result.transcript[2]["kind"], "local_exchange");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_question_without_files_needs_the_local_model() {
    let result = run_with_files(false, vec![json!({"answer": "unused"})], Ollama::Down).await;
    let error = result.exit.unwrap_err();
    assert_eq!(error.exit, Exit::Configuration);
    assert!(result.cloud_bodies.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_greeting_is_answered_locally_and_nothing_is_sent() {
    let result = run_with_files(
        false,
        Vec::new(),
        Ollama::AnswersInTurn(vec![
            json!({"reply": {"answer": "Hi! What would you like to know?"}}),
        ]),
    )
    .await;
    assert_eq!(result.exit, Ok(Exit::Success));
    assert_eq!(result.output, "Hi! What would you like to know?\n");
    assert!(
        result
            .errors
            .contains("answered by the local model; nothing was sent"),
        "{}",
        result.errors
    );
    assert!(
        result.errors.contains("cloud requests 0"),
        "{}",
        result.errors
    );
    assert!(result.cloud_bodies.is_empty());
}
