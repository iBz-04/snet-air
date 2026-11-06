use std::path::Path;

use console::Style;
use snet_protocol::CitedPassage;

use super::theme::{accent, dim};
use crate::output::Spending;

/// Who answered, shown only as the colour of the answer's dot.
#[derive(Clone, Copy)]
pub enum Answerer {
    Local,
    Cloud,
}

impl Answerer {
    fn dot(self) -> Style {
        match self {
            Answerer::Local => Style::new().green(),
            Answerer::Cloud => accent(),
        }
    }
}

/// The answer after a dot, then each cited passage quoted beneath it.
pub fn answer(text: &str, citations: &[CitedPassage], answerer: Answerer) -> Vec<String> {
    let mut lines = bulleted(&answerer.dot(), "●", text);
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

pub fn not_found(text: &str) -> Vec<String> {
    let mut lines = bulleted(
        &Style::new().yellow(),
        "●",
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

/// What `/details` shows about the last answer: who answered, what the
/// cloud read, what was spent and where the transcript is.
pub fn details(
    answered_by: &str,
    sent_question: Option<&str>,
    spending: Option<&Spending>,
    transcript: &Path,
) -> Vec<String> {
    let row = |label: &str, value: String| {
        format!("  {} {value}", dim().apply_to(format!("{label:<12}")))
    };
    let mut lines = vec![row("answered by", answered_by.to_string())];
    if let Some(question) = sent_question {
        lines.push(row("cloud read", question.to_string()));
    }
    if let Some(spending) = spending {
        let charge = spending.charge;
        lines.push(row(
            "cloud",
            format!(
                "{} · {} bytes sent · {} in / {} out tokens",
                counted(charge.requests, "request"),
                charge.bytes_sent,
                charge.input_tokens,
                charge.output_tokens
            ),
        ));
        lines.push(row("local", format!("{} tokens", spending.local_tokens)));
    }
    lines.push(row("transcript", transcript.display().to_string()));
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
        let answerer = Answerer::Cloud;
        let citations = vec![CitedPassage {
            citation: "C1".to_string(),
            document: "shift-log.txt".to_string(),
            range: 412..466,
            text: "Day 7: the inner seal of pump 3 was replaced.".to_string(),
        }];
        assert_eq!(
            plain(&answer(
                "The seal was replaced.\nOn day 7.",
                &citations,
                answerer
            )),
            vec![
                "● The seal was replaced.",
                "  On day 7.",
                "  ⎿ [C1] shift-log.txt · bytes 412..466",
                "      Day 7: the inner seal of pump 3 was replaced.",
            ]
        );
    }

    #[test]
    fn details_name_who_answered_and_where_the_transcript_is() {
        assert_eq!(
            plain(&details(
                "your machine (qwen3:4b)",
                None,
                None,
                Path::new("/t/task.jsonl")
            )),
            vec![
                "  answered by  your machine (qwen3:4b)",
                "  transcript   /t/task.jsonl"
            ]
        );
    }
}
