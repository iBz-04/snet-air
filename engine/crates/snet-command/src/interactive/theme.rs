use console::{Style, measure_text_width, style};
use dialoguer::theme::ColorfulTheme;

/// The block letters of the welcome banner, one string per row.
const BANNER: [&str; 5] = [
    " ████ █   █ █████ █████    ███  ███ ████ ",
    "█     ██  █ █       █     █   █  █  █   █",
    " ███  █ █ █ ████    █     █████  █  ████ ",
    "    █ █  ██ █       █     █   █  █  █  █ ",
    "████  █   █ █████   █     █   █ ███ █   █",
];

/// The banner rows shade from cyan to blue, top to bottom.
const BANNER_COLOURS: [u8; 5] = [51, 45, 39, 33, 27];

pub fn accent() -> Style {
    Style::new().color256(39).bold()
}

pub fn dim() -> Style {
    Style::new().dim()
}

pub fn banner() -> Vec<String> {
    BANNER
        .iter()
        .zip(BANNER_COLOURS)
        .map(|(row, colour)| format!("  {}", style(row).color256(colour)))
        .collect()
}

/// `lines` inside a rounded box as wide as the widest line.
pub fn boxed(lines: &[String]) -> Vec<String> {
    let width = lines
        .iter()
        .map(|line| measure_text_width(line))
        .max()
        .unwrap_or(0);
    let border = accent();
    let mut boxed = vec![format!(
        "{}",
        border.apply_to(format!("╭{}╮", "─".repeat(width + 2)))
    )];
    for line in lines {
        let padding = " ".repeat(width - measure_text_width(line));
        boxed.push(format!(
            "{} {line}{padding} {}",
            border.apply_to("│"),
            border.apply_to("│")
        ));
    }
    boxed.push(format!(
        "{}",
        border.apply_to(format!("╰{}╯", "─".repeat(width + 2)))
    ));
    boxed
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

/// The welcome screen: banner, then a box with the version, the working
/// directory and how to begin.
pub fn welcome(version: &str, directory: &str) -> Vec<String> {
    let mut lines = vec![String::new()];
    lines.extend(banner());
    lines.push(String::new());
    lines.extend(boxed(&[
        format!(
            "{} {}",
            accent().apply_to("✻ Welcome to snet-air"),
            dim().apply_to(version)
        ),
        String::new(),
        "A local model reads your files. A cloud model does the thinking.".to_string(),
        "Your files stay on your machine.".to_string(),
        String::new(),
        format!("{}", dim().apply_to(format!("cwd: {directory}"))),
    ]));
    lines
}

/// Menus and prompts in the session's colours.
pub fn prompts() -> ColorfulTheme {
    ColorfulTheme {
        prompt_prefix: style("?".to_string()).color256(39).bold(),
        active_item_prefix: style("❯".to_string()).color256(39).bold(),
        active_item_style: Style::new().color256(39).bold(),
        success_prefix: style("✔".to_string()).green(),
        values_style: Style::new().color256(39),
        ..ColorfulTheme::default()
    }
}

/// The question line: a `❯` and the text, kept dimmed once entered.
pub fn question_prompt() -> ColorfulTheme {
    ColorfulTheme {
        prompt_suffix: style("❯".to_string()).color256(39).bold(),
        success_suffix: style("❯".to_string()).dim(),
        values_style: Style::new(),
        ..ColorfulTheme::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_banner_row_has_the_same_width() {
        let width = BANNER[0].chars().count();
        assert!(BANNER.iter().all(|row| row.chars().count() == width));
    }

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
    fn the_box_pads_every_line_to_the_widest() {
        let lines = boxed(&["ab".to_string(), "abcd".to_string()]);
        let plain: Vec<String> = lines
            .iter()
            .map(|line| console::strip_ansi_codes(line).to_string())
            .collect();
        assert_eq!(plain, vec!["╭──────╮", "│ ab   │", "│ abcd │", "╰──────╯"]);
    }
}
