use snet_cloud::{CloudTurn, OutboundText, TaskBudget};
use snet_pseudonym::{Pseudonymiser, TaskMapping};

use crate::conversation::{CitedPassage, Conversation, ConversationOutcome, cloud_request};
use crate::errors::{ConversationError, FailureKind};
use crate::log::{self, Exchange};
use crate::messages::{self, Step};
use crate::task::ConversationTask;
use snet_protocol::{Citations, spells_out_a_value};

const MAX_CLOUD_TEXT_CHARACTERS: usize = 4_000;
const MAX_CITATIONS: usize = 32;
const MAX_QUESTION_BYTES: usize = 512;
const MAX_LOCAL_ANSWER_CHARACTERS: usize = 600;
const MAX_QUOTES: usize = 4;
const MAX_ANSWER_BYTES_PER_TASK: usize = 8 * 1024;

fn invalid_cloud(detail: impl Into<String>) -> ConversationError {
    ConversationError::new(FailureKind::InvalidCloudReply, detail)
}

/// Everything a task carries from round to round. It lives outside the
/// timed part of the run, so a timeout leaves it readable.
pub(crate) struct RoundState {
    pub(crate) mapping: TaskMapping,
    pub(crate) citations: Citations,
    pub(crate) history: Vec<CloudTurn>,
    pub(crate) answer_bytes: usize,
    pub(crate) local_tokens: u64,
    pub(crate) rounds_done: u32,
    pub(crate) in_local: Option<(u32, String)>,
}

impl RoundState {
    pub(crate) fn new() -> Self {
        Self {
            mapping: TaskMapping::new(),
            citations: Citations::default(),
            history: Vec::new(),
            answer_bytes: 0,
            local_tokens: 0,
            rounds_done: 0,
            in_local: None,
        }
    }

    pub(crate) async fn run(
        &mut self,
        conversation: &Conversation<'_>,
        task: &ConversationTask,
        budget: &TaskBudget,
        pseudonymiser: &Pseudonymiser<'_>,
        task_message: &OutboundText,
    ) -> Result<ConversationOutcome, ConversationError> {
        let rounds = task.limits.rounds;
        for round in 1..=rounds {
            let last = round == rounds;
            let schema = messages::cloud_schema(pseudonymiser, last, &self.citations.numbers())
                .map_err(|error| {
                    ConversationError::new(FailureKind::Pseudonymisation, error.to_string())
                })
                .and_then(|text| {
                    messages::cloud_reply_schema(text)
                        .map_err(|detail| ConversationError::new(FailureKind::Cloud, detail))
                })?;
            let request = cloud_request(
                pseudonymiser,
                task_message,
                &self.history,
                schema,
                conversation.cloud.config().max_output_tokens(),
            );
            let reply = conversation
                .cloud
                .send(budget, request)
                .await
                .map_err(|error| ConversationError::new(FailureKind::Cloud, error.to_string()))?;
            self.rounds_done = round;
            let step = messages::cloud_step(&reply.object).ok_or_else(|| {
                invalid_cloud("the reply's parts do not match the round's schema")
            })?;
            self.check_cloud_step(&step)?;
            let restored = self
                .mapping
                .restore(&step.text)
                .map_err(|error| invalid_cloud(error.to_string()))?;
            match step.step {
                Step::Answer => {
                    let mut citations = Vec::with_capacity(step.citations.len());
                    for name in &step.citations {
                        let cited = self.citations.find(name).ok_or_else(|| {
                            invalid_cloud("an answer cites a citation not issued")
                        })?;
                        citations.push(CitedPassage {
                            citation: name.clone(),
                            document: task.documents[cited.document].name.clone(),
                            range: cited.range.clone(),
                            text: cited.text.clone(),
                        });
                    }
                    return Ok(ConversationOutcome::Answer {
                        text: restored,
                        citations,
                    });
                }
                Step::NotFound => return Ok(ConversationOutcome::NotFound { text: restored }),
                Step::Ask if last => {
                    return Err(invalid_cloud("the last round asked another question"));
                }
                Step::Ask => {}
            }
            let report = self
                .ask_locally(conversation, task, pseudonymiser, round, restored.as_str())
                .await?;
            self.history.push(CloudTurn::Assistant(reply.turn));
            self.history.push(CloudTurn::User(report));
        }
        Err(invalid_cloud("the rounds ended without an answer"))
    }

    fn check_cloud_step(&self, step: &messages::CloudStep) -> Result<(), ConversationError> {
        let characters = step.text.chars().count();
        if characters == 0 || characters > MAX_CLOUD_TEXT_CHARACTERS || step.text.trim().is_empty()
        {
            return Err(invalid_cloud(
                "the reply's text is empty or over 4 000 characters",
            ));
        }
        match step.step {
            Step::Ask | Step::NotFound if !step.citations.is_empty() => {
                Err(invalid_cloud("a question or not_found carries citations"))
            }
            Step::Answer => {
                let mut seen = std::collections::BTreeSet::new();
                let distinct = step.citations.iter().all(|citation| seen.insert(citation));
                if step.citations.is_empty() || step.citations.len() > MAX_CITATIONS || !distinct {
                    return Err(invalid_cloud(
                        "an answer must cite 1 to 32 distinct citations",
                    ));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// One local exchange and its report.
    async fn ask_locally(
        &mut self,
        conversation: &Conversation<'_>,
        task: &ConversationTask,
        pseudonymiser: &Pseudonymiser<'_>,
        round: u32,
        question: &str,
    ) -> Result<OutboundText, ConversationError> {
        let escaped =
            serde_json::to_string(question).map_err(|error| invalid_cloud(error.to_string()))?;
        if escaped.len() - 2 > MAX_QUESTION_BYTES {
            return Err(invalid_cloud("the restored question is over 512 bytes"));
        }
        let request =
            messages::local_request(&task.documents, question, task.limits.local_output_tokens)
                .map_err(invalid_cloud)?;
        conversation
            .local
            .check(&request)
            .map_err(|error| invalid_cloud(error.to_string()))?;

        let mut exchange = Exchange {
            round,
            question: question.to_string(),
            digest: None,
            answer: None,
            error_kind: None,
            quotes: Vec::new(),
            tokens: None,
        };
        self.in_local = Some((round, question.to_string()));
        let asked = conversation.local.ask(&request).await;
        self.in_local = None;
        let answer = match asked {
            Ok(answer) => answer,
            Err(error) => {
                let failure = ConversationError::new(FailureKind::Local, error.to_string());
                return Err(self
                    .record_failed(conversation, task, exchange, failure)
                    .await);
            }
        };
        exchange.digest = Some(answer.model.digest.clone());
        exchange.tokens = Some((answer.prompt_tokens, answer.completion_tokens));
        exchange.answer = Some(answer.object.clone());
        self.local_tokens += answer.prompt_tokens + answer.completion_tokens;

        let Some(reply) = messages::local_reply(&answer.object, task.documents.len()) else {
            let failure = ConversationError::new(
                FailureKind::InvalidLocalReply,
                "the local answer's parts do not match its schema",
            );
            return Err(self
                .record_failed(conversation, task, exchange, failure)
                .await);
        };
        if reply.answer.chars().count() > MAX_LOCAL_ANSWER_CHARACTERS
            || reply.quotes.len() > MAX_QUOTES
        {
            let failure = ConversationError::new(
                FailureKind::InvalidLocalReply,
                "the local answer is over 600 characters or 4 quotes",
            );
            return Err(self
                .record_failed(conversation, task, exchange, failure)
                .await);
        }
        let mut cited = Vec::new();
        let mut unverified = 0u32;
        for (document, text) in &reply.quotes {
            let citation =
                crate::quotes::verify(&mut self.citations, &task.documents, *document, text);
            match citation {
                Some(number) if !cited.contains(&number) => cited.push(number),
                Some(_) => {}
                None => unverified += 1,
            }
            exchange
                .quotes
                .push((messages::document_id(*document), citation));
        }
        if self.citations.copied_into(&reply.answer) {
            let failure =
                ConversationError::new(FailureKind::CopiedQuote, "the local answer copies a quote");
            return Err(self
                .record_failed(conversation, task, exchange, failure)
                .await);
        }
        let pseudonymised = match pseudonymiser.text(&mut self.mapping, &reply.answer) {
            Ok(text) => text,
            Err(error) => {
                let failure =
                    ConversationError::new(FailureKind::Pseudonymisation, error.to_string());
                return Err(self
                    .record_failed(conversation, task, exchange, failure)
                    .await);
            }
        };
        if spells_out_a_value(&self.mapping, pseudonymised.as_str()) {
            let failure = ConversationError::new(
                FailureKind::SpelledValue,
                "the local answer spells out a protected value",
            );
            return Err(self
                .record_failed(conversation, task, exchange, failure)
                .await);
        }
        if self.answer_bytes + pseudonymised.as_str().len() > MAX_ANSWER_BYTES_PER_TASK {
            let failure = ConversationError::new(
                FailureKind::ReportCap,
                "the local answers would exceed 8 KiB in this task",
            );
            return Err(self
                .record_failed(conversation, task, exchange, failure)
                .await);
        }
        self.answer_bytes += pseudonymised.as_str().len();
        let final_round = round + 1 == task.limits.rounds;
        let report = messages::report(
            pseudonymiser,
            round,
            pseudonymised,
            &cited,
            unverified,
            final_round,
        )
        .map_err(|error| {
            ConversationError::new(FailureKind::Pseudonymisation, error.to_string())
        })?;
        let record = log::exchange(&task.task_id, exchange)?;
        log::append(conversation.transcript, record)?;
        Ok(report)
    }

    async fn record_failed(
        &self,
        conversation: &Conversation<'_>,
        task: &ConversationTask,
        mut exchange: Exchange,
        failure: ConversationError,
    ) -> ConversationError {
        exchange.error_kind = Some(failure.kind);
        let record = match log::exchange(&task.task_id, exchange) {
            Ok(record) => record,
            Err(error) => return error,
        };
        match log::append(conversation.transcript, record) {
            Ok(_) => failure,
            Err(error) => error,
        }
    }
}
