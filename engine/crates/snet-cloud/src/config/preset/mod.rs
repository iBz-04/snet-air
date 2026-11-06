use std::time::Duration;

use sha2::{Digest, Sha256};

use super::ceilings::{
    MAX_TASK_BYTES_SENT, MAX_TASK_COST_MICRO, MAX_TASK_REQUESTS, MAX_TASK_TOKENS,
};
use super::file::within_ceiling;
use super::wire_settings;
use super::{
    BaseUrl, EndpointConfig, InstructionRole, KeyHeader, OpenaiChatSettings, OutputCapField,
    WireFormat, WireSettings,
};
use crate::budget::{Prices, TaskLimits};
use crate::errors::ConfigError;

const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";
const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const MELIOUS_BASE_URL: &str = "https://api.melious.ai/v1";
const MAX_MODEL_LENGTH: usize = 256;

/// Who serves the cloud model, which fixes the wire format, the key header
/// and, for the two named providers, the base URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    Openai,
    Melious,
    /// Any server speaking the OpenAI-style chat format at a URL the user
    /// gives, which may sit on a private network.
    OpenaiCompatible,
}

impl Provider {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "anthropic" => Some(Self::Anthropic),
            "openai" => Some(Self::Openai),
            "melious" => Some(Self::Melious),
            "openai-compatible" => Some(Self::OpenaiCompatible),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Openai => "openai",
            Self::Melious => "melious",
            Self::OpenaiCompatible => "openai-compatible",
        }
    }

    /// The environment variable that holds this provider's API key.
    pub fn key_variable(self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::Openai => "OPENAI_API_KEY",
            Self::Melious => "MELIOUS_API_KEY",
            Self::OpenaiCompatible => "SNET_CLOUD_API_KEY",
        }
    }
}

/// What a user chooses about the cloud model; every other setting takes the
/// preset's default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetChoice {
    pub provider: Provider,
    pub model: String,
    /// Required for `OpenaiCompatible` and refused for the other providers.
    pub base_url: Option<String>,
    pub effort: String,
    pub task_limits: TaskLimits,
}

impl EndpointConfig {
    /// An endpoint built from a provider preset, with no file to write.
    pub fn preset(choice: &PresetChoice) -> Result<Self, ConfigError> {
        if choice.model.is_empty()
            || choice.model.len() > MAX_MODEL_LENGTH
            || !choice.model.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(ConfigError::invalid(
                "model",
                "it must be 1 to 256 visible ASCII characters",
            ));
        }
        let base_url_text = match (choice.provider, &choice.base_url) {
            (Provider::Anthropic, None) => ANTHROPIC_BASE_URL,
            (Provider::Openai, None) => OPENAI_BASE_URL,
            (Provider::Melious, None) => MELIOUS_BASE_URL,
            (Provider::OpenaiCompatible, Some(url)) => url.as_str(),
            (Provider::OpenaiCompatible, None) => {
                return Err(ConfigError::invalid(
                    "base_url",
                    "openai-compatible needs the server's https URL, such as https://host/v1",
                ));
            }
            (_, Some(_)) => {
                return Err(ConfigError::invalid(
                    "base_url",
                    "only openai-compatible takes a URL; the named providers use their own",
                ));
            }
        };
        let base_url = BaseUrl::parse(base_url_text)
            .map_err(|reason| ConfigError::invalid("base_url", reason))?;
        let wire = match choice.provider {
            Provider::Anthropic => WireSettings::AnthropicMessages,
            Provider::Openai => WireSettings::OpenaiChat(OpenaiChatSettings {
                key_header: KeyHeader::AuthorizationBearer,
                instruction_role: InstructionRole::Developer,
                send_store_false: true,
                output_cap_field: OutputCapField::MaxCompletionTokens,
            }),
            Provider::Melious => WireSettings::OpenaiChat(OpenaiChatSettings {
                key_header: KeyHeader::AuthorizationBearer,
                instruction_role: InstructionRole::System,
                send_store_false: false,
                output_cap_field: OutputCapField::MaxTokens,
            }),
            // Servers that copy the chat format widely accept `system` and
            // reject unknown fields such as `store`.
            Provider::OpenaiCompatible => WireSettings::OpenaiChat(OpenaiChatSettings {
                key_header: KeyHeader::AuthorizationBearer,
                instruction_role: InstructionRole::System,
                send_store_false: false,
                output_cap_field: OutputCapField::MaxCompletionTokens,
            }),
        };
        let limits = choice.task_limits;
        within_ceiling("task_limits.requests", limits.requests, MAX_TASK_REQUESTS)?;
        within_ceiling(
            "task_limits.bytes_sent",
            limits.bytes_sent,
            MAX_TASK_BYTES_SENT,
        )?;
        within_ceiling(
            "task_limits.input_tokens",
            limits.input_tokens,
            MAX_TASK_TOKENS,
        )?;
        within_ceiling(
            "task_limits.output_tokens",
            limits.output_tokens,
            MAX_TASK_TOKENS,
        )?;
        let format: WireFormat = wire.format();
        let effort = wire_settings::effort(&choice.effort, format)?;
        let description = format!(
            "{}\n{}\n{}\n{}\n{:?}",
            choice.provider.name(),
            base_url_text,
            choice.model,
            choice.effort,
            choice.task_limits
        );
        Ok(EndpointConfig {
            name: choice.provider.name().to_string(),
            base_url,
            wire,
            model: choice.model.clone(),
            effort,
            anthropic_workspace_id: None,
            allow_private_addresses: choice.provider == Provider::OpenaiCompatible,
            max_output_tokens: 8_192,
            input_token_allowance: 2_048,
            request_bytes_cap: 2 * 1024 * 1024,
            reply_bytes_cap: 4 * 1024 * 1024,
            connect_timeout: Duration::from_secs(10),
            total_timeout: Duration::from_secs(300),
            switch_starts_on: true,
            extra_ca_certificates: Vec::new(),
            replace_public_roots: false,
            // Prices are not shipped because they change too often; the
            // token and byte limits bound what a task can spend.
            prices: Prices {
                currency: "USD".to_string(),
                input_micro_per_million_tokens: 0,
                output_micro_per_million_tokens: 0,
            },
            task_limits: TaskLimits {
                cost_micro: MAX_TASK_COST_MICRO,
                ..choice.task_limits
            },
            digest: Sha256::digest(description.as_bytes()).into(),
        })
    }
}

#[cfg(test)]
mod tests;
