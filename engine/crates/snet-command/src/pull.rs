use std::io::Write;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use snet_pull::{CATALOG, DEFAULT_MODEL, Fetcher, PullError, Pulled, catalog_model, pull};

use crate::environment::Environment;
use crate::exit::{CommandError, Exit};
use crate::settings;

/// `snet pull [model]`: fetches a catalog model, with a progress bar, and
/// says what to run next.
pub async fn run(
    name: Option<&str>,
    environment: &dyn Environment,
    output: &mut dyn Write,
) -> Result<Exit, CommandError> {
    if !cfg!(feature = "gguf") {
        return Err(CommandError::usage(
            "this snet has no built-in engine to run pulled models; install a release build, or use Ollama: `ollama pull qwen3:4b`",
        ));
    }
    let name = name.unwrap_or(DEFAULT_MODEL);
    let model = catalog_model(name).ok_or_else(|| {
        let choices: Vec<&str> = CATALOG.iter().map(|model| model.name).collect();
        CommandError::usage(
            PullError::NotInCatalog {
                name: name.to_string(),
                choices: choices.join(", "),
            }
            .to_string(),
        )
    })?;
    let home = settings::home(environment)?;
    let fetcher = Fetcher::public().map_err(|error| CommandError::failed(error.to_string()))?;
    let bar = ProgressBar::new(model.bytes);
    let style = ProgressStyle::with_template(
        "{msg} {bar:32.cyan/blue} {bytes}/{total_bytes} · {bytes_per_sec} · {eta} left",
    )
    .map_err(|error| CommandError::failed(format!("the progress template: {error}")))?
    .progress_chars("━━─");
    bar.set_style(style);
    bar.set_message(format!("pulling {}", model.name));
    bar.enable_steady_tick(Duration::from_millis(200));
    let pulled = pull(&home, model, &fetcher, &mut |bytes| bar.set_position(bytes)).await;
    bar.finish_and_clear();
    let print_failed =
        |error: std::io::Error| CommandError::failed(format!("writing the result failed: {error}"));
    match pulled.map_err(|error| CommandError::failed(error.to_string()))? {
        Pulled::Downloaded(_) => {
            writeln!(output, "✔ {} is ready. Run `snet` to start.", model.name)
                .map_err(print_failed)?;
        }
        Pulled::AlreadyThere(_) => {
            writeln!(
                output,
                "✔ {} is already here. Run `snet` to start.",
                model.name
            )
            .map_err(print_failed)?;
        }
    }
    Ok(Exit::Success)
}
