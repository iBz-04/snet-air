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

use console::Term;
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
    /// The direct questions answered so far, oldest first, sent as context.
    earlier: Vec<EarlierExchange>,
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
    };
    let local = ready_local(&session, &overlay, &runtime)?;
    println!();
    print_status(&session);
    print_lines(&help_lines());
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
            Line::Help => print_lines(&help_lines()),
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
                print_status(&session);
            }
            Line::Unknown(command) => print_lines(&render::failure(&format!(
                "/{command} is not a command; type /help"
            ))),
            Line::Question { text, attach } => {
                attach_files(&mut session, attach);
                if !text.is_empty()
                    && let Some(exchange) = answer(&session, text, &overlay, &local, &runtime)
                {
                    session.remember(exchange);
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
    let local_config = settings::resolve(&session.arguments(String::new()), environment)?.local;
    let local = LocalModel::new(local_config);
    let spinner = spinner("Getting the local model ready")?;
    let ready = runtime.block_on(local.prepare());
    spinner.finish_and_clear();
    let listed =
        ready.map_err(|error| CommandError::configuration(format!("the local model: {error}")))?;
    println!(
        "  {} {}",
        accent().apply_to("✔"),
        dim().apply_to(format!("local model {} is ready", listed.name))
    );
    Ok(local)
}

/// Runs one question as `snet ask` would and prints the result. With files
/// attached the local model reads them; without, it screens and condenses
/// the question for the cloud model. Returns an exchange without files to
/// remember; a failure is printed and the session goes on.
fn answer(
    session: &Session,
    question: String,
    environment: &dyn Environment,
    local: &LocalModel,
    runtime: &tokio::runtime::Runtime,
) -> Option<EarlierExchange> {
    let direct = session.files.is_empty();
    let prepared = match ask::prepare(&session.arguments(question.clone()), environment) {
        Ok(prepared) => prepared,
        Err(error) => {
            print_lines(&render::failure(&error.to_string()));
            return None;
        }
    };
    let message = if direct {
        "Thinking: the local model reads it first"
    } else {
        "Thinking: the local model reads, the cloud model plans"
    };
    let spinner = match spinner(message) {
        Ok(spinner) => spinner,
        Err(error) => {
            print_lines(&render::failure(&error.to_string()));
            return None;
        }
    };
    let earlier: &[EarlierExchange] = if direct { &session.earlier } else { &[] };
    let finished = runtime.block_on(ask::execute(prepared, local, earlier));
    spinner.finish_and_clear();
    let mut remembered = None;
    match finished {
        Ok(Finished { ended, transcript }) => {
            if let (true, Ended::Answer { text, .. }) = (direct, &ended) {
                remembered = Some(EarlierExchange {
                    question,
                    answer: text.as_str().to_string(),
                });
            }
            let lines = match &ended {
                Ended::Answer {
                    text,
                    citations,
                    spending,
                    route,
                } => {
                    let mut lines = match route {
                        Some(Route::Local) => render::answered_locally(),
                        Some(Route::Cloud { sent_question }) => render::sent(sent_question),
                        None => Vec::new(),
                    };
                    lines.extend(render::answer(text.as_str(), citations));
                    lines.extend(render::footer(Some(spending), &transcript));
                    lines
                }
                Ended::NotFound { text, spending } => {
                    let mut lines = render::not_found(text.as_str());
                    lines.extend(render::footer(Some(spending), &transcript));
                    lines
                }
                Ended::Failed(error) => {
                    let mut lines = render::failure(&error.to_string());
                    if let Some(advice) = crate::output::failure_advice(error) {
                        lines.push(format!("  {}", dim().apply_to(advice)));
                    }
                    lines.extend(render::footer(None, &transcript));
                    lines
                }
            };
            print_lines(&lines);
        }
        Err(error) => print_lines(&render::failure(&error.to_string())),
    }
    remembered
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

fn print_status(session: &Session) {
    println!(
        "  {}",
        dim().apply_to(format!(
            "cloud {} · local {} · {}",
            session.cloud,
            session.local,
            session.mode()
        ))
    );
}

fn help_lines() -> Vec<String> {
    vec![
        format!(
            "  {}",
            dim().apply_to("Ask anything. To ask about your files, attach them with @path:")
        ),
        format!(
            "  {}",
            accent().apply_to("❯ Who signed the contract? @contract.txt")
        ),
        format!(
            "  {}",
            dim().apply_to(
                "/files list attached · /clear detach all and forget · /fanout large-file mode · /exit quit"
            )
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
