use std::sync::Arc;

use serde_json::{Value, json};
use snet_cloud::{
    ApiKey, CloudClient, CloudRequest, CloudSchema, CloudSwitch, CloudTurn, Destination,
    EndpointConfig, TaskBudget,
};

use crate::support::recorder::TestRecorder;
use crate::support::{NO_DETECTION_POLICY, outbound_with};

/// The cap that cut fan-out plans shortin an earlier live run.
const PLAN_OUTPUT_CAP: u64 = 2048;

const LIVE_ENDPOINT: &str = r#"name = "planner"
base_url = "https://api.anthropic.com/v1"
wire_format = "anthropic_messages"
anthropic_workspace = "key_scoped"
effort = "low"
model = "claude-opus-5-5"
allow_private_addresses = false
max_output_tokens = 2048
input_token_allowance = 2048
request_bytes_cap = 1048576
reply_bytes_cap = 4194304
connect_timeout_seconds = 10
total_timeout_seconds = 300
switch_starts_on = true
extra_ca_certificates = []
replace_public_roots = false

[prices]
currency = "USD"
input_micro_per_million_tokens = 5000000
output_micro_per_million_tokens = 25000000

[task_limits]
requests = 2
bytes_sent = 2097152
input_tokens = 400000
output_tokens = 40000
cost_micro = 2000000
"#;

fn plan_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "step": {"type": "string", "enum": ["plan"]},
            "jobs": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "terms": {"type": "array", "items": {"type": "string"}},
                        "task": {"type": "string"}
                    },
                    "required": ["terms", "task"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["step", "jobs"],
        "additionalProperties": false
    })
}

/// One real request, synthetic text only: a fan-out-sized planning step at
/// the cap that failed before, with low effort.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: needs ANTHROPIC_API_KEY and the network"]
async fn a_low_effort_plan_fits_the_output_cap_that_failed_before() {
    let key = ApiKey::new(std::env::var("ANTHROPIC_API_KEY").unwrap()).unwrap();
    let no_files = |_name: &std::path::Path| Err("this endpoint names no CA file".to_string());
    let config = EndpointConfig::parse(LIVE_ENDPOINT, &no_files).unwrap();
    let limits = config.task_limits();
    let destination = Destination::new("planner", "api.anthropic.com");
    let text = |text: &str| outbound_with(NO_DETECTION_POLICY, destination.clone(), text);
    let request = CloudRequest {
        instructions: text(
            "You plan work for a small local model that reads a factory's maintenance log in chunks. \
             Plan two to four small jobs. Each job names search terms that pick the chunks to read \
             and a short task the local model can do on one chunk.",
        ),
        conversation: vec![CloudTurn::User(text(
            "Question: which pumps had seal replacements last month, and were any repeated? \
             The log is 40 chunks of daily shift notes.",
        ))],
        reply_schema: CloudSchema::new("plan", text(&plan_schema().to_string())).unwrap(),
        max_output_tokens: PLAN_OUTPUT_CAP,
    };
    let recorder = Arc::new(TestRecorder::new());
    let client =
        CloudClient::new(config, key, Arc::clone(&recorder), CloudSwitch::new(true)).unwrap();
    let budget = TaskBudget::new("live-effort", limits).unwrap();

    let result = client.send(&budget, request).await;

    let sent: Value = serde_json::from_slice(&recorder.outbound()[0].body).unwrap();
    assert_eq!(sent["output_config"]["effort"], "low");
    assert_eq!(sent["max_tokens"], PLAN_OUTPUT_CAP);
    match result {
        Ok(reply) => println!(
            "accepted: model {}, {} jobs, input {} and output {} tokens",
            reply.model,
            match &reply.object["jobs"] {
                snet_cloud::JsonValue::Array(jobs) => jobs.len(),
                _ => 0,
            },
            reply.usage.input_tokens,
            reply.usage.output_tokens
        ),
        Err(error) => panic!("the live plan failed: {error}"),
    }
}
