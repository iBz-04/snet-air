use std::path::Path;

use crate::config::EndpointConfig;

pub(crate) const ANTHROPIC_ENDPOINT: &str = r#"name = "planner"
base_url = "https://api.anthropic.com/v1"
wire_format = "anthropic_messages"
anthropic_workspace = "key_scoped"
effort = "model_default"
model = "claude-opus-5-5"
allow_private_addresses = false
max_output_tokens = 8192
input_token_allowance = 2048
request_bytes_cap = 1048576
reply_bytes_cap = 4194304
connect_timeout_seconds = 10
total_timeout_seconds = 300
switch_starts_on = false
extra_ca_certificates = []
replace_public_roots = false

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
"#;

pub(crate) const OPENAI_SETTINGS: &str = r#"key_header = "authorization_bearer"
instruction_role = "developer"
send_store_false = true
"#;

/// The same endpoint speaking the OpenAI-style format: its three extra
/// settings go before the first table.
pub(crate) fn openai_endpoint() -> String {
    ANTHROPIC_ENDPOINT
        .replace(
            "wire_format = \"anthropic_messages\"",
            "wire_format = \"openai_chat\"",
        )
        .replace("anthropic_workspace = \"key_scoped\"\n", "")
        .replace("[prices]", &format!("{OPENAI_SETTINGS}\n[prices]"))
}

/// A certificate reader for endpoints that name no CA file.
pub(crate) fn no_certificate_files(_name: &Path) -> Result<Vec<u8>, String> {
    Err("this test names no certificate file".to_string())
}

pub(crate) fn load(text: &str) -> EndpointConfig {
    EndpointConfig::parse(text, &no_certificate_files).unwrap()
}

/// A policy that detects nothing, so outbound text equals its input.
pub(crate) const NO_DETECTION_POLICY: &str = "dictionaries = []\npatterns = []\npassthrough = []\n[builtin]\nEMAIL = false\nPHONE = false\nIBAN = false\nPAYMENT_CARD = false\nTR_IDENTITY = false\nIP_ADDRESS = false\nMAC_ADDRESS = false\n";

/// `text` as outbound text for the endpoint named `planner` at
/// `api.anthropic.com`.
pub(crate) fn outbound(text: &str) -> snet_pseudonym::OutboundText {
    let policy = snet_pseudonym::Policy::parse(NO_DETECTION_POLICY).unwrap();
    let destination = snet_pseudonym::Destination::new("planner", "api.anthropic.com");
    policy
        .for_destination(destination)
        .text(&mut snet_pseudonym::TaskMapping::new(), text)
        .unwrap()
}
