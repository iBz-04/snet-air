use std::path::Path;

use console::Style;
use snet_protocol::CitedPassage;

use super::theme::{accent, dim};
use crate::output::Spending;

/// The answer under a bullet, then each cited passage quoted beneath it.
pub fn answer(text: &str, citations: &[CitedPassage]) -> Vec<String> {
    let mut lines = bulleted(&accent(), "⏺", text);
    for passage in citations {
        lines.push(format!(
            "  {} {} {}",
            dim().apply_to("⎿"),
            accent().apply_to(format!("[{}]", passage.citation)),
            dim().apply_to(format!(
                "{} · bytes {}..{}",
                passage.document, passage.range.start, passage.range.end
            ))
        ));
        for line in passage.text.lines() {
            lines.push(format!("      {}", dim().apply_to(line)));
        }
    }
    lines
}

/// The question the local model wrote, as the cloud model read it.
pub fn sent(question: &str) -> Vec<String> {
    vec![format!(
        "  {} {}",
        dim().apply_to("↳ the cloud read:"),
        dim().apply_to(question)
    )]
}

/// Said when the local model answered and nothing was sent.
pub fn answered_locally() -> Vec<String> {
    vec![format!(
        "  {}",
        dim().apply_to("⌂ answered on your machine · nothing sent to the cloud")
    )]
}

pub fn not_found(text: &str) -> Vec<String> {
    let mut lines = bulleted(
        &Style::new().yellow().bold(),
        "⏺",
        "The files do not hold the answer.",
    );
    lines.extend(
        text.lines()
            .map(|line| format!("  {}", dim().apply_to(line))),
    );
    lines
}

pub fn failure(message: &str) -> Vec<String> {
    bulleted(&Style::new().red().bold(), "✗", message)
}

/// What the task spent and where its transcript is, dimmed.
pub fn footer(spending: Option<&Spending>, transcript: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(spending) = spending {
        let charge = spending.charge;
        lines.push(format!(
            "  {}",
            dim().apply_to(format!(
                "{} {} · {} · {} bytes sent · {} in / {} out cloud tokens · {} local tokens",
                spending.steps,
                spending.steps_name,
                counted(charge.requests, "cloud request"),
                charge.bytes_sent,
                charge.input_tokens,
                charge.output_tokens,
                spending.local_tokens
            ))
        ));
    }
    lines.push(format!(
        "  {}",
        dim().apply_to(format!("transcript {}", transcript.display()))
    ));
    lines
}

/// `count` and `noun`, with an `s` unless the count is one.
fn counted(count: u128, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// The first line after a coloured mark, the rest indented beneath it.
fn bulleted(mark_style: &Style, mark: &str, text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if index == 0 {
            lines.push(format!("{} {line}", mark_style.apply_to(mark)));
        } else {
            lines.push(format!("  {line}"));
        }
    }
    if lines.is_empty() {
        lines.push(format!("{}", mark_style.apply_to(mark)));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[String]) -> Vec<String> {
        lines
            .iter()
            .map(|line| console::strip_ansi_codes(line).to_string())
            .collect()
    }

    #[test]
    fn the_answer_quotes_each_cited_passage_beneath_it() {
        let citations = vec![CitedPassage {
            citation: "C1".to_string(),
            document: "shift-log.txt".to_string(),
            range: 412..466,
            text: "Day 7: the inner seal of pump 3 was replaced.".to_string(),
        }];
        assert_eq!(
            plain(&answer("The seal was replaced.\nOn day 7.", &citations)),
            vec![
                "⏺ The seal was replaced.",
                "  On day 7.",
                "  ⎿ [C1] shift-log.txt · bytes 412..466",
                "      Day 7: the inner seal of pump 3 was replaced.",
            ]
        );
    }

    #[test]
    fn the_footer_names_the_transcript_even_without_spending() {
        assert_eq!(
            plain(&footer(None, Path::new("/t/task.jsonl"))),
            vec!["  transcript /t/task.jsonl"]
        );
    }
}
