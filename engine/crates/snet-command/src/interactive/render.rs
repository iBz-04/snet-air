use std::path::Path;

use console::Style;
use snet_protocol::CitedPassage;

use super::theme::{accent, dim, failure as failure_style, warning};
use super::wordmark::Wordmark;
use crate::output::Spending;

/// The lines above an answer: who answered and with which model, and when
/// the cloud wrote it from what the local model read, which local model
/// that was.
pub fn byline(wordmark: Wordmark, model: &str, read_by: Option<&str>) -> Vec<String> {
    let mut lines = vec![format!(
        "  {}  {}",
        wordmark.styled(),
        dim().apply_to(model)
    )];
    if let Some(local_model) = read_by {
        lines.push(format!(
            "  {}",
            dim().apply_to(format!("from what {local_model} read in your files"))
        ));
    }
    lines
}

/// The byline, the answer beneath it, then each cited passage quoted.
pub fn answer(byline: &[String], text: &str, citations: &[CitedPassage]) -> Vec<String> {
    let mut lines = byline.to_vec();
    lines.extend(indented(text));
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

pub fn not_found(byline: &[String], text: &str) -> Vec<String> {
    let mut lines = byline.to_vec();
    lines.push(format!(
        "  {}",
        warning().apply_to("The files do not hold the answer.")
    ));
    lines.extend(
        text.lines()
            .map(|line| format!("  {}", dim().apply_to(line))),
    );
    lines
}

pub fn failure(message: &str) -> Vec<String> {
    bulleted(&failure_style(), "✗", message)
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

/// Every line of `text` indented under the byline.
fn indented(text: &str) -> Vec<String> {
    text.lines().map(|line| format!("  {line}")).collect()
}

/// The first line after a coloured mark, the rest indented beneath it.
fn bulleted(mark_style: &Style, mark: &str, text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if index == 0 {
            lines.push(format!("  {} {line}", mark_style.apply_to(mark)));
        } else {
            lines.push(format!("    {line}"));
        }
    }
    if lines.is_empty() {
        lines.push(format!("  {}", mark_style.apply_to(mark)));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use snet_cloud::Provider;

    fn plain(lines: &[String]) -> Vec<String> {
        lines
            .iter()
            .map(|line| console::strip_ansi_codes(line).to_string())
            .collect()
    }

    #[test]
    fn the_answer_quotes_each_cited_passage_beneath_its_byline() {
        let citations = vec![CitedPassage {
            citation: "C1".to_string(),
            document: "shift-log.txt".to_string(),
            range: 412..466,
            text: "Day 7: the inner seal of pump 3 was replaced.".to_string(),
        }];
        let byline = byline(
            Wordmark::cloud(Provider::Anthropic),
            "claude-sonnet-5-5",
            Some("qwen3:4b"),
        );
        assert_eq!(
            plain(&answer(
                &byline,
                "The seal was replaced.\nOn day 7.",
                &citations
            )),
            vec![
                "  𝗔𝗻𝘁𝗵𝗿𝗼𝗽𝗶𝗰  claude-sonnet-5-5",
                "  from what qwen3:4b read in your files",
                "  The seal was replaced.",
                "  On day 7.",
                "  ⎿ [C1] shift-log.txt · bytes 412..466",
                "      Day 7: the inner seal of pump 3 was replaced.",
            ]
        );
    }

    #[test]
    fn a_missing_answer_names_who_looked() {
        let byline = byline(Wordmark::cloud(Provider::Melious), "mistral-large", None);
        assert_eq!(
            plain(&not_found(&byline, "No line names the signer.")),
            vec![
                "  𝗠𝗲𝗹𝗶𝗼𝘂𝘀  mistral-large",
                "  The files do not hold the answer.",
                "  No line names the signer.",
            ]
        );
    }

    #[test]
    fn a_failure_is_marked_and_indented() {
        assert_eq!(
            plain(&failure("the key was refused\nset ANTHROPIC_API_KEY")),
            vec!["  ✗ the key was refused", "    set ANTHROPIC_API_KEY"]
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
