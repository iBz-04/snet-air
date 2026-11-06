use dialoguer::Select;
use dialoguer::theme::ColorfulTheme;
use snet_local::{LocalError, ollama_address, ollama_models};

use super::setup::prompt_failed;
use super::theme::dim;
use crate::environment::Environment;
use crate::exit::CommandError;
use crate::settings;

const OLLAMA_PREFIX: &str = "ollama:";

/// Takes `SNET_LOCAL` when it is set, then a model `snet pull` fetched, and
/// otherwise offers the models the local Ollama has and, in a GGUF build, a
/// GGUF file. Returns the choice as `snet ask --local` takes it. Every
/// question needs the local model, so the session cannot start without one.
pub fn choose_local(
    theme: &ColorfulTheme,
    environment: &dyn Environment,
    runtime: &tokio::runtime::Runtime,
) -> Result<String, CommandError> {
    if let Some(local) = environment.variable("SNET_LOCAL")?
        && !local.is_empty()
    {
        return Ok(local);
    }
    let pulled = settings::default_local(&settings::home(environment)?);
    if pulled != settings::DEFAULT_LOCAL {
        return Ok(pulled);
    }
    let address = ollama_address(environment.variable("OLLAMA_HOST")?.as_deref())
        .map_err(|error| CommandError::configuration(error.to_string()))?;
    let ollama = match runtime.block_on(ollama_models(address)) {
        Ok(names) => {
            if names.is_empty() {
                println!(
                    "  {}",
                    dim().apply_to("Ollama has no models yet: run `ollama pull qwen3:4b`.")
                );
            }
            names
        }
        Err(LocalError::Connect { .. }) => {
            println!(
                "  {}",
                dim().apply_to(format!(
                    "Ollama is not running at {address}: start it with `ollama serve`."
                ))
            );
            Vec::new()
        }
        Err(error) => {
            return Err(CommandError::configuration(format!(
                "listing the Ollama models failed: {error}"
            )));
        }
    };
    let labels: Vec<String> = ollama
        .iter()
        .map(|name| format!("Ollama · {name}"))
        .chain(gguf_option())
        .collect();
    if labels.is_empty() {
        return Err(CommandError::configuration(
            "no local model yet, and every question needs one: run `snet pull` (or `ollama pull qwen3:4b`)",
        ));
    }
    let index = Select::with_theme(theme)
        .with_prompt("Which local model should read your files?")
        .items(&labels)
        .default(0)
        .interact()
        .map_err(prompt_failed)?;
    match ollama.get(index) {
        Some(name) => Ok(format!("{OLLAMA_PREFIX}{name}")),
        None => gguf_path(theme),
    }
}

#[cfg(feature = "gguf")]
fn gguf_option() -> Option<String> {
    Some("quaynor · a GGUF file on your machine".to_string())
}

#[cfg(not(feature = "gguf"))]
fn gguf_option() -> Option<String> {
    None
}

#[cfg(feature = "gguf")]
fn gguf_path(theme: &ColorfulTheme) -> Result<String, CommandError> {
    let path = dialoguer::Input::<String>::with_theme(theme)
        .with_prompt("Path to the GGUF file")
        .validate_with(|text: &String| -> Result<(), String> {
            let path = std::path::Path::new(text.trim());
            if !path.is_file() {
                Err(format!("`{}` is not a file", text.trim()))
            } else if path.extension().and_then(|extension| extension.to_str()) != Some("gguf") {
                Err("the file name must end in .gguf".to_string())
            } else {
                Ok(())
            }
        })
        .interact_text()
        .map_err(prompt_failed)?;
    Ok(format!("gguf:{}", path.trim()))
}

/// Without the GGUF build there is no second option, so every index names
/// an Ollama model.
#[cfg(not(feature = "gguf"))]
fn gguf_path(_theme: &ColorfulTheme) -> Result<String, CommandError> {
    Err(CommandError::failed(
        "the local model menu returned an option it did not offer",
    ))
}
