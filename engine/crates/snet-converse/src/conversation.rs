use std::time::Duration;

use snet_cloud::{
    Charge, CloudClient, CloudRequest, CloudTurn, Destination, OutboundText, TaskBudget, TaskLimits,
};
use snet_local::LocalModel;
use snet_pseudonym::{Policy, Pseudonymiser, Restored};
use snet_transcript::Transcript;

use crate::errors::{ConversationError, FailureKind};
use crate::log::{self, Ending, Exchange};
use crate::messages;
use crate::prompts::CLOUD_PROMPT;
use crate::round::RoundState;
use crate::task::{self, ConversationTask};

const PROBE_QUESTION_BYTES: usize = 512;

pub use snet_protocol::CitedPassage;

#[derive(Debug)]
pub enum ConversationOutcome {
    Answer {
        text: Restored,
        citations: Vec<CitedPassage>,
    },
    NotFound {
        text: Restored,
    },
}

/// The end of a task that reached an answer or `not_found`.
#[derive(Debug)]
pub struct ConversationAnswer {
    pub task_id: String,
    pub outcome: ConversationOutcome,
    pub rounds: u32,
    pub charge: Charge,
    pub local_tokens: u64,
    pub task_end_line: u64,
}

/// Runs conversation tasks over one cloud endpoint, one local model, one
/// pseudonymisation policy and one transcript.
pub struct Conversation<'a> {
    pub cloud: &'a CloudClient<Transcript>,
    pub local: &'a LocalModel,
    pub policy: &'a Policy,
    pub transcript: &'a Transcript,
    /// The task's cloud budget, never wider than the endpoint's task limits;
    /// a caller may narrow it.
    pub budget: TaskLimits,
}

impl Conversation<'_> {
    /// Runs one task to its end.
    pub async fn run(
        &self,
        task: &ConversationTask,
    ) -> Result<ConversationAnswer, ConversationError> {
        let config = self.cloud.config();
        let budget = match TaskBudget::new(&task.task_id, self.budget) {
            Ok(budget) => budget,
            Err(_) => return Err(self.refuse(task, false, "task_id").await),
        };
        if !self.budget.within(&config.task_limits()) {
            return Err(self.refuse(task, true, "budget").await);
        }
        if let Some(field) = task::invalid_field(
            task,
            config.task_limits().requests,
            self.local.config().max_output_tokens(),
        ) {
            return Err(self.refuse(task, true, &field).await);
        }
        let probe = messages::local_request(
            &task.documents,
            &"q".repeat(PROBE_QUESTION_BYTES),
            task.limits.local_output_tokens,
        )
        .map_err(|detail| ConversationError::new(FailureKind::Input, detail));
        if probe
            .and_then(|request| {
                self.local
                    .check(&request)
                    .map_err(|error| ConversationError::new(FailureKind::Input, error.to_string()))
            })
            .is_err()
        {
            return Err(self.refuse(task, true, "documents").await);
        }

        let destination = Destination::new(config.name(), config.base_url().host());
        let pseudonymiser = self.policy.for_destination(destination);
        let mut state = RoundState::new();
        let question = pseudonymiser
            .text(&mut state.mapping, &task.question)
            .and_then(|question| {
                messages::task_message(
                    &pseudonymiser,
                    question,
                    task.documents.len(),
                    task.limits.rounds,
                )
            });
        let task_message = match question {
            Ok(message) => message,
            Err(error) => {
                log::append(
                    self.transcript,
                    log::refused(Some(&task.task_id), "question")?,
                )?;
                return Err(ConversationError::new(
                    FailureKind::Pseudonymisation,
                    error.to_string(),
                ));
            }
        };
        let started = log::started(self.cloud, self.local, self.policy.digest(), task)?;
        log::append(self.transcript, started)?;

        let limit = Duration::from_secs(task.limits.seconds);
        let rounds = tokio::time::timeout(
            limit,
            state.run(self, task, &budget, &pseudonymiser, &task_message),
        )
        .await;
        let result = match rounds {
            Ok(result) => result,
            Err(_elapsed) => {
                if let Some((round, question)) = state.in_local.take() {
                    let record = log::exchange(
                        &task.task_id,
                        Exchange {
                            round,
                            question,
                            digest: None,
                            answer: None,
                            error_kind: Some(FailureKind::Timeout),
                            quotes: Vec::new(),
                            tokens: None,
                        },
                    )?;
                    log::append(self.transcript, record)?;
                }
                Err(ConversationError::new(
                    FailureKind::Timeout,
                    "the task's time limit passed",
                ))
            }
        };
        self.finish(task, &budget, &state, result).await
    }

    async fn refuse(
        &self,
        task: &ConversationTask,
        task_id_valid: bool,
        field: &str,
    ) -> ConversationError {
        let task_id = task_id_valid.then_some(task.task_id.as_str());
        let record = match log::refused(task_id, field) {
            Ok(record) => record,
            Err(error) => return error,
        };
        match log::append(self.transcript, record) {
            Ok(_) => ConversationError::new(
                FailureKind::Input,
                format!("the field `{field}` is invalid"),
            ),
            Err(error) => error,
        }
    }

    async fn finish(
        &self,
        task: &ConversationTask,
        budget: &TaskBudget,
        state: &RoundState,
        result: Result<ConversationOutcome, ConversationError>,
    ) -> Result<ConversationAnswer, ConversationError> {
        if let Err(error) = &result
            && error.kind == FailureKind::Record
        {
            return Err(error.clone());
        }
        let charge = budget
            .charged()
            .map_err(|error| ConversationError::new(FailureKind::Cloud, error.to_string()))?;
        let (outcome_name, answer_text, citations, error_kind) = match &result {
            Ok(ConversationOutcome::Answer { text, citations }) => {
                ("answer", Some(text.as_str()), citations.as_slice(), None)
            }
            Ok(ConversationOutcome::NotFound { text }) => {
                ("not_found", Some(text.as_str()), &[][..], None)
            }
            Err(error) => ("failed", None, &[][..], Some(error.kind)),
        };
        let record = log::ended(
            &task.task_id,
            Ending {
                outcome: outcome_name,
                error_kind,
                answer: answer_text,
                citations,
                charge,
                local_tokens: state.local_tokens,
            },
        )?;
        let task_end_line = log::append(self.transcript, record)?;
        let outcome = result?;
        Ok(ConversationAnswer {
            task_id: task.task_id.clone(),
            outcome,
            rounds: state.rounds_done,
            charge,
            local_tokens: state.local_tokens,
            task_end_line,
        })
    }
}

pub(crate) fn cloud_request(
    pseudonymiser: &Pseudonymiser<'_>,
    task_message: &OutboundText,
    history: &[CloudTurn],
    schema: snet_cloud::CloudSchema,
    max_output_tokens: u64,
) -> CloudRequest {
    let mut conversation = Vec::with_capacity(history.len() + 1);
    conversation.push(CloudTurn::User(task_message.clone()));
    conversation.extend(history.iter().cloned());
    CloudRequest {
        instructions: pseudonymiser.template(CLOUD_PROMPT),
        conversation,
        reply_schema: schema,
        max_output_tokens,
    }
}
