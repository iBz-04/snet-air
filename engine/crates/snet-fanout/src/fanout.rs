use std::time::Duration;

use snet_cloud::{Charge, CloudClient, Destination, TaskBudget, TaskLimits};
use snet_local::LocalModel;
use snet_protocol::{CitedPassage, Ending, FailureKind, ProtocolError, append, ended, refused};
use snet_pseudonym::{Policy, Restored, fold_for_matching};
use snet_transcript::Transcript;

use crate::chunks::{chunk_size, escaped_len, split};
use crate::log::{self, Start};
use crate::messages::{self, local_request};
use crate::plan::{Chunk, Material};
use crate::rounds::{RoundState, RunInputs};
use crate::task::{self, FanOutTask};

const MIN_CHUNK_SIZE: usize = 256;
const PROBE_QUESTION_QUOTES: usize = 600;
const CONTROL_PROBE_QUESTION: &str = "q";

#[derive(Debug)]
pub enum FanOutOutcome {
    Answer {
        text: Restored,
        citations: Vec<CitedPassage>,
    },
    NotFound {
        text: Restored,
    },
}

/// The end of a fan-out task that reached an answer or `not_found`.
#[derive(Debug)]
pub struct FanOutAnswer {
    pub task_id: String,
    pub outcome: FanOutOutcome,
    pub cloud_requests: u32,
    pub local_requests: u64,
    pub local_tokens: u64,
    pub charge: Charge,
    pub task_end_line: u64,
}

/// Runs fan-out tasks over one cloud endpoint, one local model, one site
/// policy and one transcript.
pub struct FanOut<'a> {
    pub cloud: &'a CloudClient<Transcript>,
    pub local: &'a LocalModel,
    pub policy: &'a Policy,
    pub transcript: &'a Transcript,
    /// The task's cloud budget, never wider than the endpoint's task limits;
    /// a caller narrows it to enforce daily limits.
    pub budget: TaskLimits,
}

impl FanOut<'_> {
    /// Runs one task to its end.
    pub async fn run(&self, task: &FanOutTask) -> Result<FanOutAnswer, ProtocolError> {
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
        let output_tokens = task.limits.local_output_tokens;
        let probe_question = "\"".repeat(PROBE_QUESTION_QUOTES);
        let size = chunk_size(|size| {
            local_request(&"\"".repeat(size / 2), &probe_question, output_tokens)
                .is_ok_and(|request| self.local.check(&request).is_ok())
        });
        let Some(size) = size.filter(|size| *size >= MIN_CHUNK_SIZE) else {
            return Err(self.refuse(task, true, "documents").await);
        };
        let mut chunks = Vec::new();
        let mut chunk_counts = Vec::with_capacity(task.documents.len());
        for (document, item) in task.documents.iter().enumerate() {
            let ranges = split(&item.text, size);
            chunk_counts.push(ranges.len() as u64);
            for (position, range) in ranges.into_iter().enumerate() {
                let text = &item.text[range.clone()];
                let fits = local_request(text, CONTROL_PROBE_QUESTION, output_tokens)
                    .is_ok_and(|request| self.local.check(&request).is_ok());
                if !fits {
                    return Err(self
                        .refuse(task, true, &format!("documents[{document}].text"))
                        .await);
                }
                chunks.push(Chunk {
                    document,
                    number: position + 1,
                    range,
                    folded: fold_for_matching(text).to_string(),
                });
            }
        }
        let material = Material { chunks };
        let longest = material
            .chunks
            .iter()
            .map(|chunk| &task.documents[chunk.document].text[chunk.range.clone()])
            .max_by_key(|text| escaped_len(text))
            .map(str::to_string);
        let Some(longest) = longest else {
            return Err(self.refuse(task, true, "documents").await);
        };

        let destination = Destination::new(config.name(), config.base_url().host());
        let pseudonymiser = self.policy.for_destination(destination);
        let mut state = RoundState::new(task.limits.local_requests);
        let counts: Vec<u32> = chunk_counts.iter().map(|count| *count as u32).collect();
        let message = pseudonymiser
            .text(&mut state.mapping, &task.question)
            .and_then(|question| {
                messages::task_message(
                    &pseudonymiser,
                    question,
                    &counts,
                    task.limits.plan_rounds,
                    task.limits.local_requests as u32,
                )
            });
        let task_message = match message {
            Ok(message) => message,
            Err(error) => {
                append(self.transcript, refused(Some(&task.task_id), "question")?)?;
                return Err(ProtocolError::new(
                    FailureKind::Pseudonymisation,
                    error.to_string(),
                ));
            }
        };
        let start = Start {
            cloud: self.cloud,
            local: self.local,
            policy_digest: self.policy.digest(),
            chunk_size: size,
            chunk_counts,
        };
        append(self.transcript, log::started(start, task)?)?;

        let limit = Duration::from_secs(task.limits.seconds);
        let result = match tokio::time::timeout(
            limit,
            state.run(RunInputs {
                fan_out: self,
                task,
                budget: &budget,
                pseudonymiser: &pseudonymiser,
                task_message: &task_message,
                material: &material,
                longest_chunk: &longest,
            }),
        )
        .await
        {
            Ok(result) => result,
            Err(_elapsed) => Err(ProtocolError::new(
                FailureKind::Timeout,
                "the task's time limit passed",
            )),
        };
        if let Err(error) = &result {
            if error.kind == FailureKind::Record {
                return Err(error.clone());
            }
            state.record_unfinished_round(self.transcript, task, &material, error.kind)?;
        }
        self.finish(task, &budget, &state, result).await
    }

    async fn refuse(&self, task: &FanOutTask, task_id_valid: bool, field: &str) -> ProtocolError {
        let task_id = task_id_valid.then_some(task.task_id.as_str());
        let record = match refused(task_id, field) {
            Ok(record) => record,
            Err(error) => return error,
        };
        match append(self.transcript, record) {
            Ok(_) => ProtocolError::new(
                FailureKind::Input,
                format!("the field `{field}` is invalid"),
            ),
            Err(error) => error,
        }
    }

    async fn finish(
        &self,
        task: &FanOutTask,
        budget: &TaskBudget,
        state: &RoundState,
        result: Result<FanOutOutcome, ProtocolError>,
    ) -> Result<FanOutAnswer, ProtocolError> {
        let charge = budget
            .charged()
            .map_err(|error| ProtocolError::new(FailureKind::Cloud, error.to_string()))?;
        let (outcome_name, answer, citations, error_kind) = match &result {
            Ok(FanOutOutcome::Answer { text, citations }) => {
                ("answer", Some(text.as_str()), citations.as_slice(), None)
            }
            Ok(FanOutOutcome::NotFound { text }) => {
                ("not_found", Some(text.as_str()), &[][..], None)
            }
            Err(error) => ("failed", None, &[][..], Some(error.kind)),
        };
        let record = ended(
            &task.task_id,
            Ending {
                outcome: outcome_name,
                error_kind,
                answer,
                citations,
                charge,
                local_tokens: state.local_tokens,
            },
        )?;
        let task_end_line = append(self.transcript, record)?;
        let outcome = result?;
        Ok(FanOutAnswer {
            task_id: task.task_id.clone(),
            outcome,
            cloud_requests: state.cloud_requests,
            local_requests: task.limits.local_requests - state.local_requests_left,
            local_tokens: state.local_tokens,
            charge,
            task_end_line,
        })
    }
}
