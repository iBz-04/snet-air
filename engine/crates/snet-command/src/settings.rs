use std::path::PathBuf;

use snet_cloud::{ApiKey, PresetChoice, Provider, TaskLimits};
use snet_converse::ConversationLimits;
use snet_fanout::FanOutLimits;
use snet_local::{LocalBounds, LocalConfig};

use crate::arguments::AskArguments;
use crate::environment::Environment;
use crate::exit::CommandError;

pub const DEFAULT_CLOUD: &str = "anthropic:claude-sonnet-5-5";
pub const DEFAULT_LOCAL: &str = "ollama:qwen3:4b";
const OLLAMA_PREFIX: &str = "ollama:";
const DEFAULT_CONTEXT_TOKENS: u64 = 8_192;
const DEFAULT_ROUNDS: u32 = 4;
const DEFAULT_PLAN_ROUNDS: u32 = 2;
const LOCAL_OUTPUT_TOKENS: u64 = 1_024;

/// The cloud limits of one task, checked before each request is sent.
pub const TASK_LIMITS: TaskLimits = TaskLimits {
    requests: 12,
    bytes_sent: 2 * 1024 * 1024,
    input_tokens: 400_000,
    output_tokens: 64_000,
    cost_micro: 0,
};

/// Everything one `snet ask` needs, resolved from its arguments, the
/// environment and the defaults.
pub struct Settings {
    pub cloud: PresetChoice,
    pub api_key: ApiKey,
    pub local: LocalConfig,
    pub policy: Option<PathBuf>,
    pub home: PathBuf,
    pub fanout: bool,
    pub conversation: ConversationLimits,
    pub fan_out: FanOutLimits,
}

pub fn resolve(
    arguments: &AskArguments,
    environment: &dyn Environment,
) -> Result<Settings, CommandError> {
    let cloud_text = match &arguments.cloud {
        Some(text) => text.clone(),
        None => environment
            .variable("SNET_CLOUD")?
            .unwrap_or_else(|| DEFAULT_CLOUD.to_string()),
    };
    let (provider_name, model) = cloud_text.split_once(':').ok_or_else(|| {
        CommandError::usage(format!(
            "the cloud model `{cloud_text}` must be provider:model, such as {DEFAULT_CLOUD}"
        ))
    })?;
    let provider = Provider::parse(provider_name).ok_or_else(|| {
        CommandError::usage(format!(
            "the cloud provider `{provider_name}` is not anthropic, openai or openai-compatible"
        ))
    })?;
    let base_url = match (&arguments.cloud_url, provider) {
        (Some(url), _) => Some(url.clone()),
        (None, Provider::OpenaiCompatible) => environment.variable("SNET_CLOUD_URL")?,
        (None, _) => None,
    };
    let effort = match (&arguments.effort, provider) {
        (Some(effort), _) => effort.clone(),
        // Servers that copy the chat format may refuse an effort field.
        (None, Provider::OpenaiCompatible) => "model_default".to_string(),
        (None, _) => "low".to_string(),
    };
    let cloud = PresetChoice {
        provider,
        model: model.to_string(),
        base_url,
        effort,
        task_limits: TASK_LIMITS,
    };
    let key_variable = provider.key_variable();
    let key_text = environment
        .variable(key_variable)?
        .filter(|key| !key.is_empty())
        .ok_or_else(|| {
            CommandError::configuration(format!(
                "set {key_variable} to your {} API key",
                provider.name()
            ))
        })?;
    let api_key = ApiKey::new(key_text).map_err(|error| {
        CommandError::configuration(format!("{key_variable} is not a usable key: {error}"))
    })?;

    let local_text = match &arguments.local {
        Some(text) => text.clone(),
        None => environment
            .variable("SNET_LOCAL")?
            .unwrap_or_else(|| DEFAULT_LOCAL.to_string()),
    };
    let local_model = local_text.strip_prefix(OLLAMA_PREFIX).ok_or_else(|| {
        CommandError::usage(format!(
            "the local model `{local_text}` must be ollama:<model>, such as {DEFAULT_LOCAL}"
        ))
    })?;
    let ollama_host = environment.variable("OLLAMA_HOST")?;
    let local = LocalConfig::ollama(local_model, ollama_host.as_deref())
        .map_err(|error| CommandError::configuration(error.to_string()))?;
    let bounds = LocalBounds {
        context_tokens: arguments.context.unwrap_or(DEFAULT_CONTEXT_TOKENS),
        max_output_tokens: LOCAL_OUTPUT_TOKENS,
        ..local.bounds()
    };
    let local = local
        .with_bounds(bounds)
        .map_err(|error| CommandError::usage(error.to_string()))?;

    let home = match environment.variable("SNET_HOME")? {
        Some(home) if !home.is_empty() => PathBuf::from(home),
        _ => environment
            .home_directory()
            .map(|home| home.join(".snet"))
            .ok_or_else(|| {
                CommandError::configuration(
                    "the home directory is unknown; set SNET_HOME to a directory for transcripts",
                )
            })?,
    };

    let rounds = arguments.rounds.unwrap_or(DEFAULT_ROUNDS);
    if !(2..=8).contains(&rounds) {
        return Err(CommandError::usage("--rounds must be 2 to 8"));
    }
    let plan_rounds = arguments.plan_rounds.unwrap_or(DEFAULT_PLAN_ROUNDS);
    if !(1..=4).contains(&plan_rounds) {
        return Err(CommandError::usage("--plan-rounds must be 1 to 4"));
    }
    Ok(Settings {
        cloud,
        api_key,
        local,
        policy: arguments.policy.clone(),
        home,
        fanout: arguments.fanout,
        conversation: ConversationLimits {
            rounds,
            seconds: 600,
            local_output_tokens: LOCAL_OUTPUT_TOKENS,
        },
        fan_out: FanOutLimits {
            plan_rounds,
            seconds: 3_600,
            local_output_tokens: 512,
            local_requests: 512,
        },
    })
}

#[cfg(test)]
mod tests;
