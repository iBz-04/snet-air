use dialoguer::theme::ColorfulTheme;
use dialoguer::{Input, Password, Select};
use snet_cloud::Provider;

use super::overlay::OverlayEnvironment;
use super::theme::dim;
use crate::environment::Environment;
use crate::exit::CommandError;
use crate::settings::DEFAULT_CLOUD;

/// The cloud the session talks to, as `snet ask` takes it.
pub struct CloudChoice {
    pub cloud: String,
    pub cloud_url: Option<String>,
}

struct CloudOption {
    provider: Provider,
    label: &'static str,
    default_model: Option<&'static str>,
    hint: Option<&'static str>,
}

const CLOUD_OPTIONS: [CloudOption; 4] = [
    CloudOption {
        provider: Provider::Anthropic,
        label: "Anthropic",
        default_model: Some("claude-sonnet-5-5"),
        hint: None,
    },
    CloudOption {
        provider: Provider::Melious,
        label: "Melious",
        default_model: None,
        hint: Some("Pick a model that supports JSON schema output: https://melious.ai/hub/models"),
    },
    CloudOption {
        provider: Provider::Openai,
        label: "OpenAI",
        default_model: None,
        hint: None,
    },
    CloudOption {
        provider: Provider::OpenaiCompatible,
        label: "OpenAI-compatible server",
        default_model: None,
        hint: Some("Any https server that speaks the OpenAI chat format."),
    },
];

pub fn prompt_failed(error: dialoguer::Error) -> CommandError {
    CommandError::failed(format!("the terminal prompt failed: {error}"))
}

/// Takes `SNET_CLOUD` when it is set, and otherwise asks which cloud and
/// model to use; then makes sure the provider's key is known.
pub fn choose_cloud(
    theme: &ColorfulTheme,
    overlay: &mut OverlayEnvironment,
) -> Result<CloudChoice, CommandError> {
    let choice = match overlay.variable("SNET_CLOUD")? {
        Some(cloud) if !cloud.is_empty() => CloudChoice {
            cloud,
            cloud_url: overlay.variable("SNET_CLOUD_URL")?,
        },
        _ => ask_cloud(theme, overlay)?,
    };
    let provider_name = choice
        .cloud
        .split_once(':')
        .map(|(name, _)| name)
        .unwrap_or(DEFAULT_CLOUD);
    let provider = Provider::parse(provider_name).ok_or_else(|| {
        CommandError::usage(format!(
            "SNET_CLOUD names the provider `{provider_name}`, which is not anthropic, openai, melious or openai-compatible"
        ))
    })?;
    ensure_key(theme, overlay, provider)?;
    Ok(choice)
}

fn ask_cloud(
    theme: &ColorfulTheme,
    overlay: &OverlayEnvironment,
) -> Result<CloudChoice, CommandError> {
    let mut labels = Vec::with_capacity(CLOUD_OPTIONS.len());
    let mut first_with_key = None;
    for (index, option) in CLOUD_OPTIONS.iter().enumerate() {
        let variable = option.provider.key_variable();
        if has_value(overlay, variable)? {
            first_with_key.get_or_insert(index);
            labels.push(format!(
                "{}  {}",
                option.label,
                dim().apply_to(format!("{variable} found"))
            ));
        } else {
            labels.push(option.label.to_string());
        }
    }
    let index = Select::with_theme(theme)
        .with_prompt("Which cloud model should do the thinking?")
        .items(&labels)
        .default(first_with_key.unwrap_or(0))
        .interact()
        .map_err(prompt_failed)?;
    let option = &CLOUD_OPTIONS[index];
    if let Some(hint) = option.hint {
        println!("  {}", dim().apply_to(hint));
    }
    let mut model_input = Input::<String>::with_theme(theme)
        .with_prompt("Model")
        .validate_with(|text: &String| -> Result<(), &str> {
            if text.trim().is_empty() || text.trim().contains(char::is_whitespace) {
                Err("a model ID is one word, such as the one the provider lists")
            } else {
                Ok(())
            }
        });
    if let Some(model) = option.default_model {
        model_input = model_input.default(model.to_string());
    }
    let model = model_input.interact_text().map_err(prompt_failed)?;
    let cloud_url = match option.provider {
        Provider::OpenaiCompatible => Some(
            Input::<String>::with_theme(theme)
                .with_prompt("Server URL")
                .validate_with(|text: &String| -> Result<(), &str> {
                    if text.trim().starts_with("https://") {
                        Ok(())
                    } else {
                        Err("the URL must start with https://, such as https://host/v1")
                    }
                })
                .interact_text()
                .map_err(prompt_failed)?
                .trim()
                .to_string(),
        ),
        _ => None,
    };
    Ok(CloudChoice {
        cloud: format!("{}:{}", option.provider.name(), model.trim()),
        cloud_url,
    })
}

fn ensure_key(
    theme: &ColorfulTheme,
    overlay: &mut OverlayEnvironment,
    provider: Provider,
) -> Result<(), CommandError> {
    let variable = provider.key_variable();
    if has_value(overlay, variable)? {
        return Ok(());
    }
    let key = Password::with_theme(theme)
        .with_prompt(format!(
            "Paste your {} API key (kept in memory only)",
            provider.name()
        ))
        .interact()
        .map_err(prompt_failed)?;
    if key.trim().is_empty() {
        return Err(CommandError::configuration(format!(
            "no key was given; set {variable} or paste the key when asked"
        )));
    }
    overlay.set(variable, key.trim().to_string());
    println!(
        "  {}",
        dim().apply_to(format!(
            "Tip: export {variable}=... to skip this next time."
        ))
    );
    Ok(())
}

fn has_value(environment: &dyn Environment, variable: &str) -> Result<bool, CommandError> {
    Ok(environment
        .variable(variable)?
        .is_some_and(|value| !value.is_empty()))
}
