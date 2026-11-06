use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use snet_cloud::{ApiKey, CloudClient, CloudSwitch, EndpointConfig};
use snet_local::{LocalBounds, LocalConfig, LocalModel};
use snet_pseudonym::Policy;
use snet_relay::{EarlierExchange, Relay, RelayTask};
use snet_testkit::certificates::{CA_FILE, TestPki, Validity};
use snet_testkit::http::{http_response, json_response};
use snet_testkit::loopback::{LoopbackHandler, LoopbackReply, LoopbackServer};
use snet_testkit::tls::{Behaviour, TestServer, TlsVersion};
use snet_transcript::Transcript;

/// What the cloud server answers to its n-th request: a reply object, or
/// no reply at all.
pub enum Cloud {
    Reply(Value),
    Stall,
}

pub const DIGEST: &str = "3a256cc5d3b69933c87c12e52ef330494a3f91361020dffc51819c867088b8e0";

/// What the local model answers to its n-th chat request.
pub enum Local {
    Screened(Value),
    Stall,
}

pub fn screened(cloud_question: &str, private_terms: &[(&str, &str)]) -> Local {
    let terms: Vec<Value> = private_terms
        .iter()
        .map(|(category, text)| json!({"category": category, "text": text}))
        .collect();
    Local::Screened(json!({"cloud_question": cloud_question, "private_terms": terms}))
}

/// The check pass's reply: what private details are still in the question.
pub fn checked(private_terms: &[(&str, &str)]) -> Local {
    let terms: Vec<Value> = private_terms
        .iter()
        .map(|(category, text)| json!({"category": category, "text": text}))
        .collect();
    Local::Screened(json!({"private_terms": terms}))
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

fn tags() -> Vec<u8> {
    let body = json!({"models": [{"name": "local-small:latest", "model": "local-small:latest", "digest": DIGEST, "size": 1}]});
    http_response(
        200,
        &[("content-type", "application/json")],
        body.to_string().as_bytes(),
    )
}

fn cloud_reply(object: &Value) -> Vec<u8> {
    json_response(
        &json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "test-model-served",
            "content": [{"type": "text", "text": object.to_string()}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 100, "output_tokens": 30}
        })
        .to_string(),
    )
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
    pub async fn start(cloud: Vec<Cloud>, local: Vec<Local>) -> Self {
        let pki = TestPki::new().unwrap();
        let script: Arc<Vec<Option<Vec<u8>>>> = Arc::new(
            cloud
                .into_iter()
                .map(|step| match step {
                    Cloud::Reply(object) => Some(cloud_reply(&object)),
                    Cloud::Stall => None,
                })
                .collect(),
        );
        let behaviour =
            Behaviour::Handle(Arc::new(move |index, _request| match script.get(index) {
                Some(Some(bytes)) => Some(bytes.clone()),
                Some(None) => None,
                None => Some(http_response(500, &[], b"no more scripted replies")),
            }));
        let identity = pki
            .server_identity(&["127.0.0.1"], Validity::Current)
            .unwrap();
        let cloud_server = TestServer::start(identity, TlsVersion::Tls13, behaviour)
            .await
            .unwrap();
        let local_script: Arc<Vec<Option<Vec<u8>>>> = Arc::new(
            local
                .into_iter()
                .map(|answer| match answer {
                    Local::Screened(object) => Some(local_reply(&object)),
                    Local::Stall => None,
                })
                .collect(),
        );
        let chats = Arc::new(AtomicUsize::new(0));
        let handler: LoopbackHandler = Arc::new(move |request| {
            if request.request_line.starts_with("GET /api/tags") {
                return LoopbackReply::Bytes(tags());
            }
            match local_script.get(chats.fetch_add(1, Ordering::SeqCst)) {
                Some(Some(bytes)) => LoopbackReply::Bytes(bytes.clone()),
                Some(None) => LoopbackReply::Stall,
                None => LoopbackReply::Bytes(http_response(500, &[], b"{}")),
            }
        });
        let local_server = LoopbackServer::start(handler).await.unwrap();
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
        Self {
            _pki: pki,
            _directory: directory,
            cloud_server,
            local_server,
            transcript,
            cloud,
            local,
            policy: Policy::builtin().unwrap(),
        }
    }

    pub fn relay(&self) -> Relay<'_> {
        Relay {
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

    /// Every chat body the local server received, as text.
    pub fn local_bodies(&self) -> Vec<String> {
        self.local_server
            .observations
            .requests()
            .into_iter()
            .filter(|request| request.request_line.starts_with("POST /api/chat"))
            .map(|request| String::from_utf8(request.body).unwrap())
            .collect()
    }

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
}

pub fn task(question: &str, earlier: &[(&str, &str)], seconds: u64) -> RelayTask {
    RelayTask {
        task_id: "task-1".to_string(),
        question: question.to_string(),
        earlier: earlier
            .iter()
            .map(|(question, answer)| EarlierExchange {
                question: question.to_string(),
                answer: answer.to_string(),
            })
            .collect(),
        seconds,
        local_output_tokens: 128,
    }
}

pub fn endpoint_text(port: u16) -> String {
    format!(
        r#"name = "test-endpoint"
base_url = "https://127.0.0.1:{port}/v1"
wire_format = "anthropic_messages"
anthropic_workspace = "key_scoped"
effort = "model_default"
model = "test-model"
allow_private_addresses = true
max_output_tokens = 8192
input_token_allowance = 2048
request_bytes_cap = 1048576
reply_bytes_cap = 4194304
connect_timeout_seconds = 5
total_timeout_seconds = 3
switch_starts_on = true
extra_ca_certificates = ["{CA_FILE}"]
replace_public_roots = true

[prices]
currency = "USD"
input_micro_per_million_tokens = 5000000
output_micro_per_million_tokens = 25000000

[task_limits]
requests = 8
bytes_sent = 2097152
input_tokens = 400000
output_tokens = 40000
cost_micro = 2000000
"#
    )
}
