/// One line typed at the session prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Empty,
    /// A question, the files it attaches, or both. Either may be empty.
    Question {
        text: String,
        attach: Vec<String>,
    },
    Help,
    Files,
    Details,
    Clear,
    Fanout,
    Exit,
    Unknown(String),
}

/// Reads one prompt line: a slash command, or a question whose `@path`
/// words (or `@"path with spaces"`) attach files.
pub fn parse(line: &str) -> Line {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Line::Empty;
    }
    if let Some(command) = trimmed.strip_prefix('/') {
        return match command.trim() {
            "help" | "?" => Line::Help,
            "files" => Line::Files,
            "details" => Line::Details,
            "clear" => Line::Clear,
            "fanout" => Line::Fanout,
            "exit" | "quit" => Line::Exit,
            other => Line::Unknown(other.to_string()),
        };
    }
    let mut words = Vec::new();
    let mut attach = Vec::new();
    let mut rest = trimmed;
    while !rest.is_empty() {
        rest = rest.trim_start();
        if let Some(quoted) = rest.strip_prefix("@\"")
            && let Some(end) = quoted.find('"')
        {
            attach.push(quoted[..end].to_string());
            rest = &quoted[end + 1..];
            continue;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &rest[..end];
        match word.strip_prefix('@') {
            Some(path) if !path.is_empty() => attach.push(path.to_string()),
            _ if !word.is_empty() => words.push(word),
            _ => {}
        }
        rest = &rest[end..];
    }
    Line::Question {
        text: words.join(" "),
        attach,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question(text: &str, attach: &[&str]) -> Line {
        Line::Question {
            text: text.to_string(),
            attach: attach.iter().map(|path| path.to_string()).collect(),
        }
    }

    #[test]
    fn at_words_attach_files_and_leave_the_question() {
        assert_eq!(
            parse("Who signed @contract.txt and when?"),
            question("Who signed and when?", &["contract.txt"])
        );
        assert_eq!(
            parse(r#"@"my notes.txt" @logs/a.log what failed"#),
            question("what failed", &["my notes.txt", "logs/a.log"])
        );
        assert_eq!(parse("@a.txt"), question("", &["a.txt"]));
        assert_eq!(parse("mail me @ noon"), question("mail me @ noon", &[]));
    }

    #[test]
    fn slash_commands_and_blank_lines_are_read() {
        assert_eq!(parse("   "), Line::Empty);
        assert_eq!(parse("/help"), Line::Help);
        assert_eq!(parse("/files"), Line::Files);
        assert_eq!(parse("/details"), Line::Details);
        assert_eq!(parse("/clear"), Line::Clear);
        assert_eq!(parse("/fanout"), Line::Fanout);
        assert_eq!(parse("/quit"), Line::Exit);
        assert_eq!(parse("/model"), Line::Unknown("model".to_string()));
    }
}
