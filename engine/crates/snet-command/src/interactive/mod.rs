//! `snet` with no arguments in a terminal: a welcome screen, guided setup
//! and a prompt that answers questions about attached files, through the
//! same settings, checks and protocols as `snet ask`.

mod local;
mod overlay;
mod prompt;
mod render;
mod setup;
mod theme;

use std::io::ErrorKind;
use std::path::Path;
use std::time::Duration;

use console::{Style, Term};
use dialoguer::{BasicHistory, Input};
use indicatif::{ProgressBar, ProgressStyle};
use snet_local::LocalModel;
use snet_relay::{EarlierExchange, MAX_EARLIER_EXCHANGES, Route};

use crate::arguments::AskArguments;
use crate::ask::{self, Ended, Finished};
use crate::environment::Environment;
use crate::exit::{CommandError, Exit};
use crate::settings;
use overlay::OverlayEnvironment;
use prompt::Line;
use theme::{accent, dim};

const SPINNER_FRAMES: [&str; 7] = ["✻", "✳", "✢", "·", "✢", "✳", "✻"];

/// What stays the same between the questions of one session.
struct Session {
    cloud: String,
    cloud_url: Option<String>,
    local: String,
    files: Vec<String>,
    fanout: bool,
    /// The questions without files answered so far, oldest first, which
    /// the local model reads as context.
    earlier: Vec<EarlierExchange>,
    /// What `/details` shows about the last answer.
    details: Vec<String>,
}

impl Session {
    fn arguments(&self, question: String) -> AskArguments {
        AskArguments {
            fanout: self.fanout,
            cloud: Some(self.cloud.clone()),
            cloud_url: self.cloud_url.clone(),
            local: Some(self.local.clone()),
            question,
            files: self.files.clone(),
            ..AskArguments::default()
        }
    }

    fn remember(&mut self, exchange: EarlierExchange) {
        self.earlier.push(exchange);
        if self.earlier.len() > MAX_EARLIER_EXCHANGES {
            self.earlier.remove(0);
        }
    }

    /// The local model's name, without its `ollama:` or `gguf:` prefix.
    fn local_model_name(&self) -> &str {
        self.local
            .split_once(':')
            .map_or(self.local.as_str(), |(_, name)| name)
    }

    /// The cloud model's name, without its provider prefix.
    fn cloud_model_name(&self) -> &str {
        self.cloud
            .split_once(':')
            .map_or(self.cloud.as_str(), |(_, name)| name)
    }

    fn mode(&self) -> &'static str {
        if self.fanout {
            "fan-out"
        } else {
            "conversation"
        }
    }
}

/// Whether both standard input and standard output are terminals, which
/// the session needs for its menus.
pub fn available() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

pub fn run(environment: &dyn Environment) -> Result<Exit, CommandError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| CommandError::failed(format!("the runtime did not start: {error}")))?;
    let directory = std::env::current_dir()
        .map(|path| path.display().to_string())
        .map_err(|error| CommandError::failed(format!("the working directory: {error}")))?;
    let home = environment
        .home_directory()
        .map(|home| home.display().to_string());
    let directory = theme::short_directory(&directory, home.as_deref());
    print_lines(&theme::welcome(
        &format!("v{}", env!("CARGO_PKG_VERSION")),
        &directory,
    ));
    println!();

    let menus = theme::prompts();
    let mut overlay = OverlayEnvironment::new(environment);
    let cloud = setup::choose_cloud(&menus, &mut overlay)?;
    let local_text = local::choose_local(&menus, &overlay, &runtime)?;
    let mut session = Session {
        cloud: cloud.cloud,
        cloud_url: cloud.cloud_url,
        local: local_text,
        files: Vec::new(),
        fanout: false,
        earlier: Vec::new(),
        details: Vec::new(),
    };
    let local = ready_local(&session, &overlay, &runtime)?;
    println!();
    println!(
        "  {}",
        dim().apply_to("Ask anything. Attach files with @file. /help for more.")
    );
    println!();

    let questions = theme::question_prompt();
    let mut history = BasicHistory::new().max_entries(100).no_duplicates(true);
    loop {
        let typed = Input::<String>::with_theme(&questions)
            .allow_empty(true)
            .history_with(&mut history)
            .interact_text();
        let line = match typed {
            Ok(line) => line,
            Err(dialoguer::Error::IO(error))
                if matches!(
                    error.kind(),
                    ErrorKind::Interrupted | ErrorKind::UnexpectedEof
                ) =>
            {
                break;
            }
            Err(error) => return Err(setup::prompt_failed(error)),
        };
        match prompt::parse(&line) {
            Line::Empty => {}
            Line::Exit => break,
            Line::Help => print_lines(&help_lines(&session)),
            Line::Details => {
                if session.details.is_empty() {
                    println!("  {}", dim().apply_to("Nothing answered yet."));
                }
                print_lines(&session.details);
            }
            Line::Files => print_files(&session),
            Line::Clear => {
                session.files.clear();
                session.earlier.clear();
                println!(
                    "  {}",
                    dim().apply_to("No files attached, and earlier questions are forgotten.")
                );
            }
            Line::Fanout => {
                session.fanout = !session.fanout;
                println!(
                    "  {}",
                    dim().apply_to(format!("Files are now read in {} mode.", session.mode()))
                );
            }
            Line::Unknown(command) => print_lines(&render::failure(&format!(
                "/{command} is not a command; type /help"
            ))),
            Line::Question { text, attach } => {
                attach_files(&mut session, attach);
                if !text.is_empty() {
                    let (exchange, details) = answer(&session, text, &overlay, &local, &runtime);
                    if let Some(exchange) = exchange {
                        session.remember(exchange);
                    }
                    if !details.is_empty() {
                        session.details = details;
                    }
                }
            }
        }
        println!();
    }
    let _cursor_restored = Term::stdout().show_cursor();
    println!("  {}", dim().apply_to("Bye."));
    Ok(Exit::Success)
}

/// Loads the chosen local model once for the whole session.
fn ready_local(
    session: &Session,
    environment: &dyn Environment,
    runtime: &tokio::runtime::Runtime,
) -> Result<LocalModel, CommandError> {
    let settings = settings::resolve(&session.arguments(String::new()), environment)?;
    let local = LocalModel::new(settings.local.clone());
    let spinner = spinner("Loading the local model")?;
    // One screening pass now loads the weights and fills the runtime's
    // prompt cache, so the first question does not wait for either.
    let ready = runtime.block_on(async {
        let listed = local.prepare().await.map_err(|error| error.to_string())?;
        snet_relay::prime(&local, settings.conversation.local_output_tokens)
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>(listed)
    });
    spinner.finish_and_clear();
    let listed =
        ready.map_err(|error| CommandError::configuration(format!("the local model: {error}")))?;
    println!(
        "{} {}",
        Style::new().green().apply_to("✔"),
        dim().apply_to(format!("{} is ready", listed.name))
    );
    Ok(local)
}

/// Runs one question as `snet ask` would and prints the answer. With files
/// attached the local model reads them; without, it answers itself or
/// hands a screened question to the cloud. Returns an exchange without
/// files to remember and the lines `/details` shows; a failure is printed
/// and the session goes on.
fn answer(
    session: &Session,
    question: String,
    environment: &dyn Environment,
    local: &LocalModel,
    runtime: &tokio::runtime::Runtime,
) -> (Option<EarlierExchange>, Vec<String>) {
    let direct = session.files.is_empty();
    let prepared = match ask::prepare(&session.arguments(question.clone()), environment) {
        Ok(prepared) => prepared,
        Err(error) => {
            print_lines(&render::failure(&error.to_string()));
            return (None, Vec::new());
        }
    };
    let spinner = match spinner("Thinking") {
        Ok(spinner) => spinner,
        Err(error) => {
            print_lines(&render::failure(&error.to_string()));
            return (None, Vec::new());
        }
    };
    let earlier: &[EarlierExchange] = if direct { &session.earlier } else { &[] };
    let finished = runtime.block_on(ask::execute(prepared, local, earlier));
    spinner.finish_and_clear();
    let Finished { ended, transcript } = match finished {
        Ok(finished) => finished,
        Err(error) => {
            print_lines(&render::failure(&error.to_string()));
            return (None, Vec::new());
        }
    };
    let local_name = format!("your machine ({})", session.local_model_name());
    let cloud_name = format!("the cloud ({})", session.cloud_model_name());
    let mut remembered = None;
    let details = match &ended {
        Ended::Answer {
            text,
            citations,
            spending,
            route,
        } => {
            if direct {
                remembered = Some(EarlierExchange {
                    question,
                    answer: text.as_str().to_string(),
                });
            }
            let (answerer, answered_by, sent) = match route {
                Some(Route::Local) => (render::Answerer::Local, local_name, None),
                Some(Route::Cloud { sent_question }) => (
                    render::Answerer::Cloud,
                    cloud_name,
                    Some(sent_question.as_str()),
                ),
                None => (
                    render::Answerer::Cloud,
                    format!("{cloud_name}, from what {local_name} read in your files"),
                    None,
                ),
            };
            print_lines(&render::answer(text.as_str(), citations, answerer));
            render::details(&answered_by, sent, Some(spending), &transcript)
        }
        Ended::NotFound { text, spending } => {
            print_lines(&render::not_found(text.as_str()));
            render::details(&cloud_name, None, Some(spending), &transcript)
        }
        Ended::Failed(error) => {
            let mut lines = render::failure(&error.to_string());
            if let Some(advice) = crate::output::failure_advice(error) {
                lines.push(format!("  {}", dim().apply_to(advice)));
            }
            print_lines(&lines);
            render::details("nobody: the task failed", None, None, &transcript)
        }
    };
    (remembered, details)
}

fn attach_files(session: &mut Session, attach: Vec<String>) {
    for path in attach {
        if !Path::new(&path).is_file() {
            print_lines(&render::failure(&format!("`{path}` is not a file")));
        } else if session.files.contains(&path) {
            println!(
                "  {}",
                dim().apply_to(format!("{path} is already attached"))
            );
        } else {
            println!("  {} {}", accent().apply_to("+"), path);
            session.files.push(path);
        }
    }
}

fn print_files(session: &Session) {
    if session.files.is_empty() {
        println!(
            "  {}",
            dim().apply_to("No files attached. Attach one with @path.")
        );
    }
    for file in &session.files {
        println!("  {} {file}", accent().apply_to("•"));
    }
}

fn help_lines(session: &Session) -> Vec<String> {
    let row = |command: &str, meaning: &str| {
        format!(
            "  {} {}",
            accent().apply_to(format!("{command:<10}")),
            dim().apply_to(meaning)
        )
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
        format!(
            "  {} answered on your machine   {} answered by the cloud",
            Style::new().green().apply_to("●"),
            accent().apply_to("●")
        ),
        format!(
            "  {}",
            dim().apply_to(format!("local {} · cloud {}", session.local, session.cloud))
        ),
    ]
}

fn spinner(message: &'static str) -> Result<ProgressBar, CommandError> {
    let style = ProgressStyle::with_template("{spinner:.cyan} {msg} {elapsed:.dim}")
        .map_err(|error| CommandError::failed(format!("the spinner template: {error}")))?
        .tick_strings(&SPINNER_FRAMES);
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(style);
    spinner.set_message(message);
    spinner.enable_steady_tick(Duration::from_millis(120));
    Ok(spinner)
}

fn print_lines(lines: &[String]) {
    for line in lines {
        println!("{line}");
    }
}
