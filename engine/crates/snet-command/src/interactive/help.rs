use snet_cloud::Provider;

use super::theme::{accent, dim};
use super::wordmark::Wordmark;

/// What `/help` shows: the commands, which wordmark means the local model
/// and which the session's cloud, and the models in use.
pub fn lines(provider: Provider, local: &str, cloud: &str) -> Vec<String> {
    let row = |command: &str, meaning: &str| {
        format!(
            "  {} {}",
            accent().apply_to(format!("{command:<10}")),
            dim().apply_to(meaning)
        )
    };
    let answerer = |wordmark: Wordmark, meaning: &str| {
        format!("  {}  {}", wordmark.styled(), dim().apply_to(meaning))
    };
    vec![
        row("@file", "attach a file; only your machine reads it"),
        row(
            "/details",
            "who answered the last question, and what the cloud read",
        ),
        row("/files", "list attached files"),
        row("/clear", "detach all files and forget earlier questions"),
        row("/fanout", "read large files in chunks"),
        row("/exit", "quit"),
        String::new(),
        answerer(Wordmark::local(), "answered on your machine"),
        answerer(Wordmark::cloud(provider), "answered in the cloud"),
        format!(
            "  {}",
            dim().apply_to(format!("local {local} · cloud {cloud}"))
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interactive::wordmark::sans_serif_bold;

    #[test]
    fn help_tells_the_local_wordmark_from_the_cloud_one() {
        let plain: Vec<String> = lines(
            Provider::Melious,
            "ollama:qwen3:4b",
            "melious:mistral-large",
        )
        .iter()
        .map(|line| console::strip_ansi_codes(line).to_string())
        .collect();
        let local_label = sans_serif_bold(if cfg!(target_os = "macos") {
            "This Mac"
        } else {
            "This computer"
        });
        assert_eq!(
            plain[plain.len() - 3..],
            [
                format!("  {local_label}  answered on your machine"),
                "  𝗠𝗲𝗹𝗶𝗼𝘂𝘀  answered in the cloud".to_string(),
                "  local ollama:qwen3:4b · cloud melious:mistral-large".to_string(),
            ]
        );
    }
}
