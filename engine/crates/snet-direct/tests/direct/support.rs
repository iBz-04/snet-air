use std::sync::Arc;

use serde_json::{Value, json};
use snet_cloud::{ApiKey, CloudClient, CloudSwitch, EndpointConfig};
use snet_direct::{Direct, DirectTask, EarlierExchange};
use snet_pseudonym::Policy;
use snet_testkit::certificates::{CA_FILE, TestPki, Validity};
use snet_testkit::http::{http_response, json_response};
use snet_testkit::tls::{Behaviour, TestServer, TlsVersion};
use snet_transcript::Transcript;

/// What the cloud server answers to its n-th request: a reply object, or
/// no reply at all.
pub enum Cloud {
    Reply(Value),
    Stall,
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
    pub transcript: Transcript,
    pub cloud: CloudClient<Transcript>,
    pub policy: Policy,
}

impl Rig {
    pub async fn start(cloud: Vec<Cloud>) -> Self {
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
            transcript,
            cloud,
            policy: Policy::builtin().unwrap(),
        }
    }

    pub fn direct(&self) -> Direct<'_> {
        Direct {
            cloud: &self.cloud,
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

pub fn task(question: &str, earlier: &[(&str, &str)], seconds: u64) -> DirectTask {
    DirectTask {
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
