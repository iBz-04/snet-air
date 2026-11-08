use console::{Style, measure_text_width};
use dialoguer::theme::ColorfulTheme;

use super::palette::{APPLE_BLUE, APPLE_GREEN, APPLE_GREY, APPLE_ORANGE, APPLE_RED};

/// The one accent colour: commands, menu choices and the prompt mark.
pub fn accent() -> Style {
    APPLE_BLUE.style().bold()
}

/// Secondary text: hints, paths, model names and quoted passages.
pub fn dim() -> Style {
    APPLE_GREY.style()
}

pub fn success() -> Style {
    APPLE_GREEN.style()
}

pub fn warning() -> Style {
    APPLE_ORANGE.style()
}

pub fn failure() -> Style {
    APPLE_RED.style().bold()
}

/// Directories longer than this are shown by their last characters.
const DIRECTORY_WIDTH: usize = 56;

/// `directory` with the home directory written `~`, cut from the left to
/// fit the welcome box.
pub fn short_directory(directory: &str, home: Option<&str>) -> String {
    let directory = match home {
        Some(home)
            if !home.is_empty()
                && directory
                    .strip_prefix(home)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('/')) =>
        {
            format!("~{}", &directory[home.len()..])
        }
        _ => directory.to_string(),
    };
    let length = directory.chars().count();
    if length <= DIRECTORY_WIDTH {
        return directory;
    }
    let tail: String = directory
        .chars()
        .skip(length - (DIRECTORY_WIDTH - 1))
        .collect();
    format!("…{tail}")
}

const PURPOSE: &str = "A local model reads your files. A cloud model does the thinking.";

/// The welcome header: the name and version, what snet does, the working
/// directory and a hairline as wide as the widest of them.
pub fn welcome(version: &str, directory: &str) -> Vec<String> {
    let rows = [
        format!(
            "{}  {}",
            Style::new().bold().apply_to("snet-air"),
            dim().apply_to(version)
        ),
        PURPOSE.to_string(),
        format!("{}", dim().apply_to(directory)),
    ];
    let width = rows
        .iter()
        .map(|row| measure_text_width(row))
        .max()
        .unwrap_or(0);
    let mut lines = vec![String::new()];
    lines.extend(rows.iter().map(|row| format!("  {row}")));
    lines.push(format!("  {}", dim().apply_to("─".repeat(width))));
    lines
}

/// Menus and prompts in the session's colours. They draw on standard
/// error, so each style follows standard error's colour setting.
pub fn prompts() -> ColorfulTheme {
    ColorfulTheme {
        prompt_prefix: accent().for_stderr().apply_to("?".to_string()),
        hint_style: dim().for_stderr(),
        defaults_style: dim().for_stderr(),
        active_item_prefix: accent().for_stderr().apply_to("›".to_string()),
        active_item_style: accent().for_stderr(),
        picked_item_prefix: accent().for_stderr().apply_to("›".to_string()),
        success_prefix: success().for_stderr().apply_to("✓".to_string()),
        success_suffix: dim().for_stderr().apply_to("·".to_string()),
        error_prefix: failure().for_stderr().apply_to("✗".to_string()),
        error_style: failure().for_stderr(),
        values_style: APPLE_BLUE.style().for_stderr(),
        ..ColorfulTheme::default()
    }
}

/// The question line: a `›` and the text, kept dimmed once entered.
pub fn question_prompt() -> ColorfulTheme {
    ColorfulTheme {
        prompt_suffix: accent().for_stderr().apply_to("›".to_string()),
        success_suffix: dim().for_stderr().apply_to("›".to_string()),
        values_style: Style::new().for_stderr(),
        ..ColorfulTheme::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_directories_are_shortened_from_the_left() {
        assert_eq!(short_directory("/home/me/work", Some("/home/me")), "~/work");
        assert_eq!(short_directory("/home/meg", Some("/home/me")), "/home/meg");
        let long = format!("/data/{}", "x".repeat(80));
        let short = short_directory(&long, None);
        assert_eq!(short.chars().count(), DIRECTORY_WIDTH);
        assert!(short.starts_with('…') && short.ends_with('x'));
    }

    #[test]
    fn the_header_names_the_version_and_directory_over_a_hairline() {
        let plain: Vec<String> = welcome("v1.2.3", "~/work")
            .iter()
            .map(|line| console::strip_ansi_codes(line).to_string())
            .collect();
        assert_eq!(
            plain,
            vec![
                String::new(),
                "  snet-air  v1.2.3".to_string(),
                format!("  {PURPOSE}"),
                "  ~/work".to_string(),
                format!("  {}", "─".repeat(PURPOSE.chars().count())),
            ]
        );
    }
}
