pub mod recorder;

pub use snet_testkit::certificates;

pub mod server {
    pub use snet_testkit::http::{http_response, json_response};
    pub use snet_testkit::tls::*;
}

use std::sync::Arc;

use serde_json::json;
use snet_cloud::{
    ApiKey, CloudClient, CloudError, CloudRequest, CloudSchema, CloudSwitch, CloudTurn,
    Destination, EndpointConfig, Failure, OutboundText, TaskBudget, WireFormat,
};
use snet_pseudonym::{Policy, TaskMapping};

use certificates::{CA_FILE, TestPki, Validity};
use recorder::TestRecorder;
use server::{Behaviour, TestServer, TlsVersion};

/// Never printed by any assertion; tests search every record, error and
/// `Debug` string for it.
pub const KEY_MARKER: &str = "SECRETKEYBYTES";

pub fn key() -> ApiKey {
    ApiKey::new(format!("sk-test-{KEY_MARKER}-0123456789")).unwrap()
}

pub fn endpoint_text(format: WireFormat, port: u16) -> String {
    let format_lines = match format {
        WireFormat::AnthropicMessages => "wire_format = \"anthropic_messages\"\nanthropic_workspace = \"key_scoped\"\neffort = \"model_default\"\n".to_string(),
        WireFormat::OpenaiChat => "wire_format = \"openai_chat\"\neffort = \"model_default\"\nkey_header = \"authorization_bearer\"\ninstruction_role = \"developer\"\nsend_store_false = true\n".to_string(),
    };
    format!(
        r#"name = "test-endpoint"
base_url = "https://127.0.0.1:{port}/v1"
{format_lines}model = "test-model"
allow_private_addresses = true
max_output_tokens = 8192
input_token_allowance = 2048
request_bytes_cap = 1048576
reply_bytes_cap = 4194304
connect_timeout_seconds = 5
total_timeout_seconds = 20
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

/// Replaces the whole line that sets `key`.
pub fn set(text: &str, key: &str, value: &str) -> String {
    let prefix = format!("{key} = ");
    let mut replaced = false;
    let lines: Vec<String> = text
        .lines()
        .map(|line| {
            if line.starts_with(&prefix) {
                replaced = true;
                format!("{key} = {value}")
            } else {
                line.to_string()
            }
        })
        .collect();
    assert!(replaced, "no line sets {key}");
    lines.join("\n") + "\n"
}

/// A policy that detects nothing, so outbound text equals its input.
pub const NO_DETECTION_POLICY: &str = "dictionaries = []\npatterns = []\npassthrough = []\n[builtin]\nEMAIL = false\nPHONE = false\nIBAN = false\nPAYMENT_CARD = false\nTR_IDENTITY = false\nIP_ADDRESS = false\nMAC_ADDRESS = false\n";

/// The endpoint every harness configures.
pub fn test_destination() -> Destination {
    Destination::new("test-endpoint", "127.0.0.1")
}

/// `text` as outbound text for `destination` under `policy`.
pub fn outbound_with(policy: &str, destination: Destination, text: &str) -> OutboundText {
    Policy::parse(policy)
        .unwrap()
        .for_destination(destination)
        .text(&mut TaskMapping::new(), text)
        .unwrap()
}

pub fn outbound(text: &str) -> OutboundText {
    outbound_with(NO_DETECTION_POLICY, test_destination(), text)
}

pub fn answer_schema_for(destination: Destination) -> CloudSchema {
    let schema = json!({
        "type": "object",
        "properties": {"answer": {"type": "string"}},
        "required": ["answer"],
        "additionalProperties": false
    });
    CloudSchema::new(
        "answer",
        outbound_with(NO_DETECTION_POLICY, destination, &schema.to_string()),
    )
    .unwrap()
}

pub fn request() -> CloudRequest {
    request_for(test_destination())
}

/// The standard request, made for `destination`.
pub fn request_for(destination: Destination) -> CloudRequest {
    let text = |text: &str| outbound_with(NO_DETECTION_POLICY, destination.clone(), text);
    CloudRequest {
        instructions: text("Answer from the local findings only."),
        conversation: vec![CloudTurn::User(text("Which seal failed on pump 3?"))],
        reply_schema: answer_schema_for(destination.clone()),
        max_output_tokens: 1000,
    }
}

pub fn anthropic_reply(output_tokens: u64) -> String {
    json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "test-model-served",
        "content": [{"type": "text", "text": "{\"answer\":\"the inner seal\"}"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 100, "output_tokens": output_tokens}
    })
    .to_string()
}

pub fn openai_reply() -> String {
    json!({
        "id": "chatcmpl-1",
        "model": "test-model-served",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "{\"answer\":\"the inner seal\"}", "refusal": null},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 100, "completion_tokens": 30}
    })
    .to_string()
}

/// A server, a client for it and everything a test inspects afterwards.
pub struct Harness {
    pub server: TestServer,
    pub switch: CloudSwitch,
    pub recorder: Arc<TestRecorder>,
    pub client: CloudClient<TestRecorder>,
    pub budget: TaskBudget,
}

/// Configures the test recorder once the server and switch exist.
pub type RecorderSetup = Box<dyn FnOnce(TestRecorder, &HarnessParts) -> TestRecorder>;

pub struct HarnessOptions {
    pub format: WireFormat,
    pub tls: TlsVersion,
    pub behaviour: Behaviour,
    pub edit: Box<dyn FnOnce(String) -> String>,
    pub recorder: RecorderSetup,
    pub identity: IdentityChoice,
    pub switch_source: Option<Arc<dyn snet_cloud::SwitchSource>>,
}

pub struct HarnessParts {
    pub server: Arc<server::Observations>,
    pub switch: CloudSwitch,
}

#[derive(Clone, Copy)]
pub enum IdentityChoice {
    Trusted,
    OtherName,
    Expired,
    Untrusted,
}

impl HarnessOptions {
    pub fn replying(format: WireFormat, body: Vec<u8>) -> Self {
        Self {
            format,
            tls: TlsVersion::Tls13,
            behaviour: Behaviour::Respond(body),
            edit: Box::new(|text| text),
            recorder: Box::new(|recorder, _parts| recorder),
            identity: IdentityChoice::Trusted,
            switch_source: None,
        }
    }

    pub fn anthropic_ok() -> Self {
        Self::replying(
            WireFormat::AnthropicMessages,
            server::json_response(&anthropic_reply(30)),
        )
    }
}

impl Harness {
    pub async fn start(options: HarnessOptions) -> Self {
        let pki = TestPki::new().unwrap();
        let names = ["127.0.0.1"];
        let identity = match options.identity {
            IdentityChoice::Trusted => pki.server_identity(&names, Validity::Current).unwrap(),
            IdentityChoice::OtherName => pki
                .server_identity(&["other.example"], Validity::Current)
                .unwrap(),
            IdentityChoice::Expired => pki.server_identity(&names, Validity::Expired).unwrap(),
            IdentityChoice::Untrusted => pki.untrusted_identity(&names).unwrap(),
        };
        let server = TestServer::start(identity, options.tls, options.behaviour)
            .await
            .unwrap();
        let text = (options.edit)(endpoint_text(options.format, server.port));
        let config = EndpointConfig::parse(
            &text,
            &snet_testkit::certificates::read_from(pki.directory.path()),
        )
        .unwrap();
        let switch = match options.switch_source {
            Some(source) => CloudSwitch::with_source(config.switch_starts_on(), source),
            None => CloudSwitch::new(config.switch_starts_on()),
        };
        let parts = HarnessParts {
            server: Arc::clone(&server.observations),
            switch: switch.clone(),
        };
        let recorder = Arc::new((options.recorder)(TestRecorder::new(), &parts));
        let budget = TaskBudget::new("task-1", config.task_limits()).unwrap();
        let client =
            CloudClient::new(config, key(), Arc::clone(&recorder), switch.clone()).unwrap();
        Self {
            server,
            switch,
            recorder,
            client,
            budget,
        }
    }

    pub async fn send(&self) -> Result<snet_cloud::CloudReply, CloudError> {
        self.client.send(&self.budget, request()).await
    }

    pub async fn send_failure(&self) -> Failure {
        match self.send().await {
            Ok(reply) => panic!("the request succeeded: {:?}", reply.object),
            Err(error) => {
                assert_eq!(error.endpoint, "test-endpoint");
                error.failure
            }
        }
    }
}
