use std::ffi::OsString;
use std::path::PathBuf;

use crate::exit::CommandError;

pub const USAGE: &str = "usage: snet ask [options] <question> <file>...

A model on this machine reads the files. A cloud model plans the work and
writes the answer from what the local model reports; it never sees the files.

options:
  --fanout                  split large files into chunks and run small jobs on each
  --cloud <provider:model>  anthropic:<model>, openai:<model> or openai-compatible:<model>
                            (default: $SNET_CLOUD, else anthropic:claude-sonnet-5-5)
  --cloud-url <url>         the https URL of an openai-compatible server ($SNET_CLOUD_URL)
  --local <ollama:model>    the local model (default: $SNET_LOCAL, else ollama:qwen3:4b)
  --context <tokens>        the local model's context window (default 8192)
  --effort <level>          how hard the cloud model thinks (default low)
  --rounds <n>              conversation rounds, 2 to 8 (default 4)
  --plan-rounds <n>         fan-out planning rounds, 1 to 4 (default 2)
  --policy <file>           a pseudonymisation policy file (default: built-in detectors)

environment:
  ANTHROPIC_API_KEY, OPENAI_API_KEY or SNET_CLOUD_API_KEY   the cloud API key
  OLLAMA_HOST   where Ollama listens (default 127.0.0.1:11434)
  SNET_HOME     where transcripts are written (default ~/.snet)

other commands:
  snet --help | snet --version";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AskArguments {
    pub fanout: bool,
    pub cloud: Option<String>,
    pub cloud_url: Option<String>,
    pub local: Option<String>,
    pub context: Option<u64>,
    pub effort: Option<String>,
    pub rounds: Option<u32>,
    pub plan_rounds: Option<u32>,
    pub policy: Option<PathBuf>,
    pub question: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Ask(Box<AskArguments>),
    Help,
    Version,
}

/// Reads the operating system's arguments, the program name first, refusing
/// any that is not UTF-8.
pub fn from_os(arguments: Vec<OsString>) -> Result<Command, CommandError> {
    let mut texts = Vec::with_capacity(arguments.len());
    for (position, argument) in arguments.into_iter().enumerate().skip(1) {
        let text = argument
            .into_string()
            .map_err(|_| CommandError::usage(format!("argument {position} is not UTF-8")))?;
        texts.push(text);
    }
    parse(&texts)
}

/// Parses the arguments after the program name.
pub fn parse(arguments: &[String]) -> Result<Command, CommandError> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["--help" | "-h" | "help"] => Ok(Command::Help),
        ["--version" | "-V"] => Ok(Command::Version),
        ["ask", rest @ ..] => ask(rest).map(|parsed| Command::Ask(Box::new(parsed))),
        _ => Err(CommandError::usage(USAGE)),
    }
}

fn ask(words: &[&str]) -> Result<AskArguments, CommandError> {
    let mut parsed = AskArguments::default();
    let mut positional = Vec::new();
    let mut index = 0;
    let mut options_ended = false;
    while index < words.len() {
        let word = words[index];
        index += 1;
        if options_ended || !word.starts_with("--") {
            positional.push(word.to_string());
            continue;
        }
        if word == "--" {
            options_ended = true;
            continue;
        }
        if word == "--fanout" {
            if parsed.fanout {
                return Err(repeated(word));
            }
            parsed.fanout = true;
            continue;
        }
        let Some(value) = words.get(index) else {
            return Err(CommandError::usage(format!("{word} needs a value")));
        };
        index += 1;
        let value = value.to_string();
        match word {
            "--cloud" => set(&mut parsed.cloud, word, value)?,
            "--cloud-url" => set(&mut parsed.cloud_url, word, value)?,
            "--local" => set(&mut parsed.local, word, value)?,
            "--effort" => set(&mut parsed.effort, word, value)?,
            "--policy" => set(&mut parsed.policy, word, PathBuf::from(value))?,
            "--context" => set(&mut parsed.context, word, number(word, &value)?)?,
            "--rounds" => set(&mut parsed.rounds, word, small_number(word, &value)?)?,
            "--plan-rounds" => set(&mut parsed.plan_rounds, word, small_number(word, &value)?)?,
            _ => {
                return Err(CommandError::usage(format!(
                    "unknown option {word}; see snet --help"
                )));
            }
        }
    }
    let mut positional = positional.into_iter();
    parsed.question = positional
        .next()
        .ok_or_else(|| CommandError::usage("ask needs a question and at least one file"))?;
    parsed.files = positional.collect();
    if parsed.files.is_empty() {
        return Err(CommandError::usage(
            "ask needs at least one file after the question",
        ));
    }
    for (index, file) in parsed.files.iter().enumerate() {
        if !snet_protocol::name_is_valid(file) {
            return Err(CommandError::usage(format!(
                "file {} has a name longer than 256 characters or with a control character",
                index + 1
            )));
        }
        if parsed.files[..index].contains(file) {
            return Err(CommandError::usage(format!("file `{file}` is named twice")));
        }
    }
    Ok(parsed)
}

fn repeated(option: &str) -> CommandError {
    CommandError::usage(format!("{option} is given twice"))
}

fn set<T>(slot: &mut Option<T>, option: &str, value: T) -> Result<(), CommandError> {
    if slot.is_some() {
        return Err(repeated(option));
    }
    *slot = Some(value);
    Ok(())
}

fn number(option: &str, text: &str) -> Result<u64, CommandError> {
    let is_decimal = !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    if !is_decimal {
        return Err(CommandError::usage(format!(
            "{option} takes a whole number, not `{text}`"
        )));
    }
    text.parse()
        .map_err(|_| CommandError::usage(format!("{option} `{text}` is too large")))
}

fn small_number(option: &str, text: &str) -> Result<u32, CommandError> {
    u32::try_from(number(option, text)?)
        .map_err(|_| CommandError::usage(format!("{option} `{text}` is too large")))
}

#[cfg(test)]
mod tests;
