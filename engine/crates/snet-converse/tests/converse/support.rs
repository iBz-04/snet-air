use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use snet_cloud::{ApiKey, CloudClient, CloudSwitch, EndpointConfig};
use snet_converse::{Conversation, ConversationLimits, ConversationTask, Document};
use snet_local::{LocalBounds, LocalConfig, LocalModel};
use snet_pseudonym::Policy;
use snet_testkit::certificates::{CA_FILE, TestPki, Validity};
use snet_testkit::http::{http_response, json_response};
use snet_testkit::loopback::{LoopbackHandler, LoopbackReply, LoopbackServer};
use snet_testkit::tls::{Behaviour, TestServer, TlsVersion};
use snet_transcript::Transcript;

pub const DIGEST: &str = "3a256cc5d3b69933c87c12e52ef330494a3f91361020dffc51819c867088b8e0";
pub const SHIFT_LOG: &str = "07:10 Ahmet Yılmaz reported a leak at pump 3. Call him on +90 532 123 45 67.\n09:40 Zeynep Kaya replaced the inner seal of pump 3.\n";
pub const SEAL_QUOTE: &str = "Zeynep Kaya replaced the inner seal of pump 3";

/// What the cloud server answers to its n-th request.
pub enum Cloud {
    Step(Value),
    Stall,
}

/// What the local model answers to its n-th chat request.
pub enum Local {
    Answer(Value),
    Stall,
}

pub fn ask(text: &str) -> Cloud {
    Cloud::Step(json!({"step": "ask", "text": text, "citations": []}))
}

pub fn answer(text: &str, citations: &[&str]) -> Cloud {
    Cloud::Step(json!({"step": "answer", "text": text, "citations": citations}))
}

pub fn local(answer: &str, quotes: &[(&str, &str)]) -> Local {
    let quotes: Vec<Value> = quotes
        .iter()
        .map(|(document, text)| json!({"document": document, "text": text}))
        .collect();
    Local::Answer(json!({"answer": answer, "quotes": quotes}))
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
        let cloud_script: Arc<Vec<Option<Vec<u8>>>> = Arc::new(
            cloud
                .into_iter()
                .map(|step| match step {
                    Cloud::Step(object) => Some(cloud_reply(&object)),
                    Cloud::Stall => None,
                })
                .collect(),
        );
        let behaviour = Behaviour::Handle(Arc::new(move |index, _request| {
            match cloud_script.get(index) {
                Some(Some(bytes)) => Some(bytes.clone()),
                Some(None) => None,
                None => Some(http_response(500, &[], b"no more scripted replies")),
            }
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
                    Local::Answer(object) => Some(local_reply(&object)),
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

    pub fn conversation(&self) -> Conversation<'_> {
        Conversation {
            cloud: &self.cloud,
            local: &self.local,
            policy: &self.policy,
            transcript: &self.transcript,
            budget: self.cloud.config().task_limits(),
        }
    }

    /// Every outbound body the cloud server received, as text.
    pub fn cloud_bodies(&self) -> Vec<String> {
        self.cloud_server
            .observations
            .requests()
            .into_iter()
            .map(|request| String::from_utf8(request.body).unwrap())
            .collect()
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

    /// The first transcript line of `kind`.
    pub fn line(&self, kind: &str) -> Value {
        self.records()
            .into_iter()
            .find(|record| record["kind"] == kind)
            .unwrap()
    }

    pub fn task_end(&self) -> Value {
        self.line("task_ended")
    }
}

pub fn task(question: &str, rounds: u32) -> ConversationTask {
    ConversationTask {
        task_id: "task-1".to_string(),
        question: question.to_string(),
        documents: vec![Document {
            name: "shift-log.txt".to_string(),
            text: SHIFT_LOG.to_string(),
        }],
        limits: ConversationLimits {
            rounds,
            seconds: 30,
            local_output_tokens: 128,
        },
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
