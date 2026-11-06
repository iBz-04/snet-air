use std::io::Write;
use std::sync::Arc;

use snet_cloud::{CloudClient, CloudSwitch, EndpointConfig};
use snet_converse::{Conversation, ConversationOutcome, ConversationTask};
use snet_direct::{Direct, DirectTask, EarlierExchange};
use snet_fanout::{FanOut, FanOutOutcome, FanOutTask};
use snet_local::LocalModel;
use snet_protocol::{CitedPassage, Document, ProtocolError};
use snet_pseudonym::{Policy, Restored};
use snet_transcript::Transcript;

use crate::arguments::AskArguments;
use crate::documents::{self, CONVERSATION_FILE_BYTES, FAN_OUT_FILE_BYTES};
use crate::environment::Environment;
use crate::exit::{CommandError, Exit};
use crate::output::{self, Spending};
use crate::settings::{self, Settings};

/// How a task ended.
pub(crate) enum Ended {
    Answer {
        text: Restored,
        citations: Vec<CitedPassage>,
        spending: Spending,
    },
    NotFound {
        text: Restored,
        spending: Spending,
    },
    Failed(ProtocolError),
}

/// A finished task and where its transcript was written.
pub(crate) struct Finished {
    pub(crate) ended: Ended,
    pub(crate) transcript: std::path::PathBuf,
}

/// A task ready to run: its settings, policy, files and cloud endpoint.
pub(crate) struct Prepared {
    settings: Settings,
    question: String,
    policy: Policy,
    files: Vec<Document>,
    endpoint: EndpointConfig,
}

/// `snet ask`: settings and the API key are checked before any file is read.
pub async fn run(
    arguments: &AskArguments,
    environment: &dyn Environment,
    output: &mut dyn Write,
    errors: &mut dyn Write,
) -> Result<Exit, CommandError> {
    let prepared = prepare(arguments, environment)?;
    let local = LocalModel::new(prepared.settings.local.clone());
    let finished = execute(prepared, Some(&local), &[]).await?;
    report(finished, output, errors)
}

/// Resolves the settings, loads the policy, reads the files and builds the
/// cloud endpoint, before anything runs.
pub(crate) fn prepare(
    arguments: &AskArguments,
    environment: &dyn Environment,
) -> Result<Prepared, CommandError> {
    let settings = settings::resolve(arguments, environment)?;
    let policy = match &settings.policy {
        Some(path) => Policy::load(path).map_err(|error| {
            CommandError::configuration(format!("policy `{}`: {error}", path.display()))
        })?,
        None => Policy::builtin()
            .map_err(|error| CommandError::failed(format!("the built-in policy: {error}")))?,
    };
    let limit = if settings.fanout {
        FAN_OUT_FILE_BYTES
    } else {
        CONVERSATION_FILE_BYTES
    };
    let files = arguments
        .files
        .iter()
        .map(|name| documents::read(name, limit, settings.fanout))
        .collect::<Result<Vec<_>, _>>()?;
    let endpoint = EndpointConfig::preset(&settings.cloud)
        .map_err(|error| CommandError::configuration(format!("the cloud model: {error}")))?;
    Ok(Prepared {
        settings,
        question: arguments.question.clone(),
        policy,
        files,
        endpoint,
    })
}

/// Runs one task: with files, `local` reads them (one model answers every
/// task of a session); with none, the cloud model answers directly, with
/// `earlier` exchanges of the session as context.
pub(crate) async fn execute(
    prepared: Prepared,
    local: Option<&LocalModel>,
    earlier: &[EarlierExchange],
) -> Result<Finished, CommandError> {
    let Prepared {
        settings,
        question,
        policy,
        files,
        endpoint,
    } = prepared;
    let local = if files.is_empty() {
        None
    } else {
        let local = local.ok_or_else(|| {
            CommandError::configuration("no local model is set up to read the files")
        })?;
        // Before anything goes to the cloud: Ollama answers and has the
        // model, or the GGUF file is loaded.
        local
            .prepare()
            .await
            .map_err(|error| CommandError::configuration(format!("the local model: {error}")))?;
        Some(local)
    };
    let task_id = new_task_id()?;
    let transcript = Transcript::create(&settings.home, &task_id)
        .map_err(|error| CommandError::failed(format!("the transcript: {error}")))?;
    let cloud = CloudClient::new(
        endpoint,
        settings.api_key,
        Arc::new(transcript.clone()),
        CloudSwitch::new(true),
    )
    .map_err(|error| CommandError::configuration(error.to_string()))?;
    let budget = cloud.config().task_limits();

    let ended = match local {
        None => {
            let task = DirectTask {
                task_id,
                question,
                earlier: earlier.to_vec(),
                seconds: settings.conversation.seconds,
            };
            let protocol = Direct {
                cloud: &cloud,
                policy: &policy,
                transcript: &transcript,
                budget,
            };
            match protocol.run(&task).await {
                Ok(answer) => Ended::Answer {
                    text: answer.text,
                    citations: Vec::new(),
                    spending: Spending {
                        steps: 1,
                        steps_name: "question",
                        charge: answer.charge,
                        local_tokens: 0,
                    },
                },
                Err(error) => Ended::Failed(error),
            }
        }
        Some(local) if settings.fanout => {
            let task = FanOutTask {
                task_id,
                question,
                documents: files,
                limits: settings.fan_out,
            };
            let protocol = FanOut {
                cloud: &cloud,
                local,
                policy: &policy,
                transcript: &transcript,
                budget,
            };
            match protocol.run(&task).await {
                Ok(answer) => {
                    let spending = Spending {
                        steps: answer.local_requests,
                        steps_name: "local requests",
                        charge: answer.charge,
                        local_tokens: answer.local_tokens,
                    };
                    match answer.outcome {
                        FanOutOutcome::Answer { text, citations } => Ended::Answer {
                            text,
                            citations,
                            spending,
                        },
                        FanOutOutcome::NotFound { text } => Ended::NotFound { text, spending },
                    }
                }
                Err(error) => Ended::Failed(error),
            }
        }
        Some(local) => {
            let task = ConversationTask {
                task_id,
                question,
                documents: files,
                limits: settings.conversation,
            };
            let protocol = Conversation {
                cloud: &cloud,
                local,
                policy: &policy,
                transcript: &transcript,
                budget,
            };
            match protocol.run(&task).await {
                Ok(answer) => {
                    let spending = Spending {
                        steps: u64::from(answer.rounds),
                        steps_name: "rounds",
                        charge: answer.charge,
                        local_tokens: answer.local_tokens,
                    };
                    match answer.outcome {
                        ConversationOutcome::Answer { text, citations } => Ended::Answer {
                            text,
                            citations,
                            spending,
                        },
                        ConversationOutcome::NotFound { text } => {
                            Ended::NotFound { text, spending }
                        }
                    }
                }
                Err(error) => Ended::Failed(error),
            }
        }
    };
    // A task can end on its time limit while an exchange is still running;
    // its outcome line is written before the command returns.
    cloud.exchanges_finished().await;
    Ok(Finished {
        ended,
        transcript: transcript.path().to_path_buf(),
    })
}

fn report(
    finished: Finished,
    output: &mut dyn Write,
    errors: &mut dyn Write,
) -> Result<Exit, CommandError> {
    let Finished { ended, transcript } = finished;
    let print_failed =
        |error: std::io::Error| CommandError::failed(format!("writing the result failed: {error}"));
    let exit = match ended {
        Ended::Answer {
            text,
            citations,
            spending,
        } => {
            for line in output::answer_lines(text.as_str(), &citations) {
                writeln!(output, "{line}").map_err(print_failed)?;
            }
            writeln!(errors, "{}", output::spending_line(&spending)).map_err(print_failed)?;
            Exit::Success
        }
        Ended::NotFound { text, spending } => {
            writeln!(output, "{}", text.as_str()).map_err(print_failed)?;
            writeln!(errors, "the files do not hold the answer").map_err(print_failed)?;
            writeln!(errors, "{}", output::spending_line(&spending)).map_err(print_failed)?;
            Exit::NoAnswer
        }
        Ended::Failed(error) => {
            writeln!(errors, "snet: {error}").map_err(print_failed)?;
            Exit::NoAnswer
        }
    };
    writeln!(errors, "transcript: {}", transcript.display()).map_err(print_failed)?;
    Ok(exit)
}

/// `task-` and 32 lowercase hex characters from the system random
/// generator.
fn new_task_id() -> Result<String, CommandError> {
    let mut bytes = [0u8; 16];
    aws_lc_rs::rand::fill(&mut bytes)
        .map_err(|_| CommandError::failed("the system random generator failed"))?;
    Ok(format!("task-{}", snet_protocol::hex(&bytes)))
}

#[cfg(test)]
mod tests;
