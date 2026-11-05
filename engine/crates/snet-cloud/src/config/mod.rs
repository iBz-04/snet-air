mod base_url;
pub(crate) mod ceilings;
pub(crate) mod file;
mod preset;
mod wire_settings;

use std::path::Path;
use std::time::Duration;

use rustls_pki_types::CertificateDer;

pub use base_url::BaseUrl;
pub use preset::{PresetChoice, Provider};

/// Returns the bytes of one CA file named in `extra_ca_certificates`, given
/// the name exactly as the endpoint file wrote it, or a message saying why it
/// could not.
pub type CertificateReader<'a> = &'a dyn Fn(&Path) -> Result<Vec<u8>, String>;
use serde::Deserialize;

use crate::budget::{Prices, TaskLimits};
use crate::errors::ConfigError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireFormat {
    OpenaiChat,
    AnthropicMessages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyHeader {
    AuthorizationBearer,
    ApiKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionRole {
    System,
    Developer,
}

/// How hard the cloud model should think. `ModelDefault`
/// sends no effort field, so the model's own default applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    ModelDefault,
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    /// The value the provider's field takes, or nothing to send.
    pub fn wire_value(self) -> Option<&'static str> {
        match self {
            Effort::ModelDefault => None,
            Effort::None => Some("none"),
            Effort::Minimal => Some("minimal"),
            Effort::Low => Some("low"),
            Effort::Medium => Some("medium"),
            Effort::High => Some("high"),
            Effort::Xhigh => Some("xhigh"),
            Effort::Max => Some("max"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenaiChatSettings {
    pub key_header: KeyHeader,
    pub instruction_role: InstructionRole,
    pub send_store_false: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireSettings {
    OpenaiChat(OpenaiChatSettings),
    AnthropicMessages,
}

impl WireSettings {
    pub fn format(&self) -> WireFormat {
        match self {
            Self::OpenaiChat(_) => WireFormat::OpenaiChat,
            Self::AnthropicMessages => WireFormat::AnthropicMessages,
        }
    }
}

/// One cloud endpoint, from a provider preset or an endpoint file whose
/// every field is required and has no default.
#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub(crate) name: String,
    pub(crate) base_url: BaseUrl,
    pub(crate) wire: WireSettings,
    pub(crate) model: String,
    pub(crate) effort: Effort,
    /// The workspace an Anthropic request names, or none for a key scoped to
    /// one workspace.
    pub(crate) anthropic_workspace_id: Option<String>,
    pub(crate) allow_private_addresses: bool,
    pub(crate) max_output_tokens: u64,
    pub(crate) input_token_allowance: u64,
    pub(crate) request_bytes_cap: u64,
    pub(crate) reply_bytes_cap: u64,
    pub(crate) connect_timeout: Duration,
    pub(crate) total_timeout: Duration,
    pub(crate) switch_starts_on: bool,
    pub(crate) extra_ca_certificates: Vec<CertificateDer<'static>>,
    pub(crate) replace_public_roots: bool,
    pub(crate) prices: Prices,
    pub(crate) task_limits: TaskLimits,
    pub(crate) digest: [u8; 32],
}

impl EndpointConfig {
    /// Checks an endpoint file's text and takes each CA certificate it names
    /// from `read_certificate`, which the caller supplies so that it applies
    /// its own file checks; this crate opens no file.
    pub fn parse(text: &str, read_certificate: CertificateReader<'_>) -> Result<Self, ConfigError> {
        file::parse_endpoint(text, read_certificate)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn base_url(&self) -> &BaseUrl {
        &self.base_url
    }

    pub fn wire(&self) -> WireSettings {
        self.wire
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn switch_starts_on(&self) -> bool {
        self.switch_starts_on
    }

    pub fn task_limits(&self) -> TaskLimits {
        self.task_limits
    }

    pub fn prices(&self) -> &Prices {
        &self.prices
    }

    pub fn max_output_tokens(&self) -> u64 {
        self.max_output_tokens
    }

    pub fn input_token_allowance(&self) -> u64 {
        self.input_token_allowance
    }

    /// SHA-256 over the configuration text and every extra CA certificate,
    /// written into each outbound record as the rule that allowed it.
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

#[cfg(test)]
mod tests;
