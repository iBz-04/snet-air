use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};
use snet_cloud::{ApiKey, CloudClient, CloudSwitch, EndpointConfig};
use snet_fanout::{Document, FanOut, FanOutLimits, FanOutTask};
use snet_local::{LocalBounds, LocalConfig, LocalModel};
use snet_pseudonym::Policy;
use snet_testkit::certificates::{CA_FILE, TestPki, Validity};
use snet_testkit::http::{http_response, json_response};
use snet_testkit::loopback::{LoopbackHandler, LoopbackReply, LoopbackServer};
use snet_testkit::tls::{Behaviour, TestServer, TlsVersion};
use snet_transcript::Transcript;

pub const DIGEST: &str = "3a256cc5d3b69933c87c12e52ef330494a3f91361020dffc51819c867088b8e0";
pub const SEAL_LINE: &str = "Day 7: Zeynep Kaya replaced the inner seal of pump 3.";
pub const LEAK_LINE: &str = "Day 9: Ahmet Yılmaz reported a valve leak, call +90 532 123 45 67.";

/// A month of shift log lines with the two lines that matter.
pub fn shift_log() -> String {
    let mut text = String::new();
    for day in 1..=40 {
        let line = match day {
            7 => SEAL_LINE.to_string(),
            9 => LEAK_LINE.to_string(),
            _ => format!("Day {day}: line 2 ran normally at 4.2 bar, no remarks from the crew."),
        };
        text.push_str(&line);
        text.push('\n');
    }
    text
}

pub fn maintenance() -> String {
    let mut text = String::new();
    for row in 1..=20 {
        text.push_str(&format!(
            "Order {row}: filters and belts checked, nothing replaced.\n"
        ));
    }
    text.push_str("Pump 3 seal type SE-200 was ordered on day 6.\n");
    text
}

/// What the cloud server answers to its n-th request.
pub enum Cloud {
    Step(Value),
}

pub fn plan(text: &str, jobs: &[(&str, &[&str])]) -> Cloud {
    let jobs: Vec<Value> = jobs
        .iter()
        .map(|(question, terms)| json!({"question": question, "terms": terms}))
        .collect();
    Cloud::Step(json!({"step": "plan", "text": text, "jobs": jobs, "citations": []}))
}

pub fn answer(text: &str, citations: &[&str]) -> Cloud {
    Cloud::Step(json!({"step": "answer", "text": text, "jobs": [], "citations": citations}))
}

/// What the local model does with one chunk and one job question.
pub enum Local {
    Answer(Value),
    Fail,
    Stall,
}

pub type LocalScript = Arc<dyn Fn(&str, &str) -> Local + Send + Sync>;

/// Relevant when the chunk holds `needle`, quoting the line that holds it.
pub fn quoting(needle: &'static str, finding: &'static str) -> LocalScript {
    Arc::new(
        move |chunk, _question| match chunk.lines().find(|line| line.contains(needle)) {
            Some(line) => {
                Local::Answer(json!({"relevant": true, "finding": finding, "quotes": [line]}))
            }
            None => Local::Answer(json!({"relevant": false, "finding": "", "quotes": []})),
        },
    )
}

fn cloud_reply(object: &Value) -> Vec<u8> {
    json_response(
        &json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": "test-model-served",
            "content": [{"type": "text", "text": object.to_string()}],
            "stop_reason": "end_turn", "usage": {"input_tokens": 100, "output_tokens": 30}
        })
        .to_string(),
    )
}

fn local_reply(object: &Value) -> Vec<u8> {
    let body = json!({"model": "local-small:latest", "message": {"role": "assistant", "content": object.to_string()},
        "done": true, "done_reason": "stop", "prompt_eval_count": 300, "eval_count": 40});
    http_response(
        200,
        &[("content-type", "application/json")],
        body.to_string().as_bytes(),
    )
}

fn tags(listed_name: &str) -> Vec<u8> {
    let body = json!({"models": [{"name": listed_name, "model": listed_name, "digest": DIGEST, "size": 1}]});
    http_response(
        200,
        &[("content-type", "application/json")],
        body.to_string().as_bytes(),
    )
}

/// The chunk and question of a local chat request body.
fn chunk_and_question(body: &[u8]) -> Option<(String, String)> {
    let body: Value = serde_json::from_slice(body).ok()?;
    let message: Value = serde_json::from_str(body["messages"][1]["content"].as_str()?).ok()?;
    Some((
        message["chunk"].as_str()?.to_string(),
        message["question"].as_str()?.to_string(),
    ))
}

/// The parts of a job-run transcript line the tests read.
#[derive(Debug, Deserialize)]
pub struct JobRun {
    pub job: u32,
    pub terms: Vec<String>,
    pub chunks: Vec<ChunkRun>,
}

#[derive(Debug, Deserialize)]
pub struct ChunkRun {
    pub outcome: String,
    pub error_kind: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TaskEnd {
    pub outcome: String,
    pub error_kind: Option<String>,
    pub charge: TaskCharge,
}

#[derive(Debug, Deserialize)]
pub struct TaskCharge {
    pub requests: String,
}

pub struct Rig {
    _pki: TestPki,
    _directory: tempfile::TempDir,
    pub cloud_server: TestServer,
    pub local_server: LoopbackServer,
    pub transcript: Transcript,
    pub cloud: CloudClient<Transcript>,
    pub local: LocalModel,
    pub policy: Policy,
}

impl Rig {
    pub async fn start(cloud: Vec<Cloud>, script: LocalScript) -> Self {
        Self::start_listing(cloud, script, "local-small:latest").await
    }

    /// A rig whose local server lists one model under `listed_name`.
    pub async fn start_listing(
        cloud: Vec<Cloud>,
        script: LocalScript,
        listed_name: &'static str,
    ) -> Self {
        let pki = TestPki::new().unwrap();
        let cloud_script: Arc<Vec<Vec<u8>>> = Arc::new(
            cloud
                .into_iter()
                .map(|Cloud::Step(object)| cloud_reply(&object))
                .collect(),
        );
        let behaviour = Behaviour::Handle(Arc::new(move |index, _request| {
            match cloud_script.get(index) {
                Some(bytes) => Some(bytes.clone()),
                None => Some(http_response(500, &[], b"no more scripted replies")),
            }
        }));
        let identity = pki
            .server_identity(&["127.0.0.1"], Validity::Current)
            .unwrap();
        let cloud_server = TestServer::start(identity, TlsVersion::Tls13, behaviour)
            .await
            .unwrap();
        let handler: LoopbackHandler = Arc::new(move |request| {
            if request.request_line.starts_with("GET /api/tags") {
                return LoopbackReply::Bytes(tags(listed_name));
            }
            let Some((chunk, question)) = chunk_and_question(&request.body) else {
                return LoopbackReply::Bytes(http_response(400, &[], b"{}"));
            };
            match script(&chunk, &question) {
                Local::Answer(object) => LoopbackReply::Bytes(local_reply(&object)),
                Local::Fail => LoopbackReply::Bytes(http_response(
                    500,
                    &[("content-type", "application/json")],
                    br#"{"error":"x"}"#,
                )),
                Local::Stall => LoopbackReply::Stall,
            }
        });
        let local_server = LoopbackServer::start(handler).await.unwrap();

        let directory = tempfile::tempdir().unwrap();
        std::fs::copy(pki.ca_path(), directory.path().join(CA_FILE)).unwrap();
        let endpoint = EndpointConfig::parse(
            &endpoint_text(cloud_server.port),
            &snet_testkit::certificates::read_from(directory.path()),
        )
        .unwrap();
        let transcript = Transcript::create(directory.path(), "task-1").unwrap();
        let cloud = CloudClient::new(
            endpoint,
            ApiKey::new("sk-test-not-a-real-key".to_string()).unwrap(),
            Arc::new(transcript.clone()),
            CloudSwitch::new(true),
        )
        .unwrap();
        let local = LocalModel::new(
            LocalConfig::ollama("local-small", Some(&local_server.address.to_string()))
                .unwrap()
                .with_bounds(LocalBounds {
                    context_tokens: 4096,
                    max_output_tokens: 256,
                    total_timeout_seconds: 5,
                    max_concurrent_requests: 2,
                })
                .unwrap(),
        );
        let dictionary = directory.path().join("people.txt");
        std::fs::write(&dictionary, "Ahmet Yılmaz\nZeynep Kaya\n").unwrap();
        let policy = Policy::parse(&format!(
            "patterns = []\npassthrough = []\n[builtin]\nEMAIL = true\nPHONE = true\nIBAN = true\nPAYMENT_CARD = true\nTR_IDENTITY = true\nIP_ADDRESS = true\nMAC_ADDRESS = true\n[[dictionaries]]\ncategory = \"PERSON\"\npath = \"{}\"\n",
            dictionary.display()
        ))
        .unwrap();
        Self {
            _pki: pki,
            _directory: directory,
            cloud_server,
            local_server,
            transcript,
            cloud,
            local,
            policy,
        }
    }

    pub fn fan_out(&self) -> FanOut<'_> {
        FanOut {
            cloud: &self.cloud,
            local: &self.local,
            policy: &self.policy,
            transcript: &self.transcript,
            budget: self.cloud.config().task_limits(),
        }
    }

    pub fn cloud_bodies(&self) -> Vec<String> {
        self.cloud_server
            .observations
            .requests()
            .into_iter()
            .map(|request| String::from_utf8(request.body).unwrap())
            .collect()
    }

    /// Local chat requests, leaving out the model lists.
    pub fn local_chats(&self) -> usize {
        self.local_server
            .observations
            .requests()
            .iter()
            .filter(|request| request.request_line.starts_with("POST"))
            .count()
    }

    /// Every transcript line, in order.
    pub fn records(&self) -> Vec<Value> {
        std::fs::read_to_string(self.transcript.path())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    pub fn kinds(&self) -> Vec<String> {
        self.records()
            .iter()
            .map(|record| record["kind"].as_str().unwrap().to_string())
            .collect()
    }

    pub fn task_end(&self) -> TaskEnd {
        let line = self
            .records()
            .into_iter()
            .find(|record| record["kind"] == "task_ended")
            .unwrap();
        serde_json::from_value(line).unwrap()
    }

    pub fn job_runs(&self) -> Vec<JobRun> {
        self.records()
            .into_iter()
            .filter(|record| record["kind"] == "job_run")
            .map(|line| serde_json::from_value(line).unwrap())
            .collect()
    }
}

pub fn task(question: &str, plan_rounds: u32) -> FanOutTask {
    FanOutTask {
        task_id: "task-1".to_string(),
        question: question.to_string(),
        documents: vec![
            Document {
                name: "shift-log.txt".to_string(),
                text: shift_log(),
            },
            Document {
                name: "maintenance.txt".to_string(),
                text: maintenance(),
            },
        ],
        limits: FanOutLimits {
            plan_rounds,
            seconds: 60,
            local_output_tokens: 128,
            local_requests: 200,
        },
    }
}

pub fn endpoint_text(port: u16) -> String {
    format!(
        "name = \"test-endpoint\"\nbase_url = \"https://127.0.0.1:{port}/v1\"\nwire_format = \"anthropic_messages\"\nanthropic_workspace = \"key_scoped\"\neffort = \"model_default\"\nmodel = \"test-model\"\nallow_private_addresses = true\nmax_output_tokens = 8192\ninput_token_allowance = 2048\nrequest_bytes_cap = 1048576\nreply_bytes_cap = 4194304\nconnect_timeout_seconds = 5\ntotal_timeout_seconds = 3\nswitch_starts_on = true\nextra_ca_certificates = [\"{CA_FILE}\"]\nreplace_public_roots = true\n\n[prices]\ncurrency = \"USD\"\ninput_micro_per_million_tokens = 5000000\noutput_micro_per_million_tokens = 25000000\n\n[task_limits]\nrequests = 8\nbytes_sent = 2097152\ninput_tokens = 400000\noutput_tokens = 40000\ncost_micro = 2000000\n"
    )
}
