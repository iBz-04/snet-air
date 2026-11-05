use std::sync::Mutex;

use snet_cloud::{CloudRequest, CloudTurn, OutboundText, TaskBudget};
use snet_protocol::{Citations, CitedPassage, FailureKind, ProtocolError, append};
use snet_pseudonym::{Pseudonymiser, TaskMapping};
use snet_transcript::{ChunkRunLine, QuoteLine, TaskRecord, Transcript};

use crate::fanout::{FanOut, FanOutOutcome};
use crate::filter::{ChunkOutcome, FilterState, filter_round};
use crate::log::{JobRuns, job_record};
use crate::messages::{self, Step, cloud_step, local_request};
use crate::plan::{Material, PlannedJob, check_citations, check_step, plan_jobs};
use crate::prompts::CLOUD_PROMPT;
use crate::run::{Slot, run_round};
use crate::task::FanOutTask;

fn invalid(detail: impl Into<String>) -> ProtocolError {
    ProtocolError::new(FailureKind::InvalidCloudReply, detail)
}

/// What every round of one task reads.
pub(crate) struct RunInputs<'r> {
    pub(crate) fan_out: &'r FanOut<'r>,
    pub(crate) task: &'r FanOutTask,
    pub(crate) budget: &'r TaskBudget,
    pub(crate) pseudonymiser: &'r Pseudonymiser<'r>,
    pub(crate) task_message: &'r OutboundText,
    pub(crate) material: &'r Material,
    pub(crate) longest_chunk: &'r str,
}

/// The round whose local requests are under way, kept so that any ending
/// can record them.
pub(crate) struct CurrentRound {
    pub(crate) round: u32,
    pub(crate) jobs: Vec<PlannedJob>,
    pub(crate) slots: Mutex<Vec<Slot>>,
}

/// What a task carries from round to round; it lives outside the timed
/// part of the run, so a timeout leaves it readable.
pub(crate) struct RoundState {
    pub(crate) mapping: TaskMapping,
    pub(crate) citations: Citations,
    pub(crate) verified_texts: Vec<String>,
    pub(crate) sent_bytes: usize,
    pub(crate) history: Vec<CloudTurn>,
    pub(crate) local_requests_left: u64,
    pub(crate) local_tokens: u64,
    pub(crate) cloud_requests: u32,
    pub(crate) current: Option<CurrentRound>,
}

impl RoundState {
    pub(crate) fn new(local_requests: u64) -> Self {
        Self {
            mapping: TaskMapping::new(),
            citations: Citations::default(),
            verified_texts: Vec::new(),
            sent_bytes: 0,
            history: Vec::new(),
            local_requests_left: local_requests,
            local_tokens: 0,
            cloud_requests: 0,
            current: None,
        }
    }

    pub(crate) async fn run(
        &mut self,
        inputs: RunInputs<'_>,
    ) -> Result<FanOutOutcome, ProtocolError> {
        let RunInputs {
            fan_out,
            task,
            budget,
            pseudonymiser,
            task_message,
            material,
            longest_chunk,
        } = inputs;
        let requests = task.limits.plan_rounds + 1;
        for request_number in 1..=requests {
            let last = request_number == requests;
            let schema = messages::cloud_schema(pseudonymiser, last, &self.citations.numbers())
                .map_err(|error| {
                    ProtocolError::new(FailureKind::Pseudonymisation, error.to_string())
                })
                .and_then(|text| {
                    messages::cloud_reply_schema(text)
                        .map_err(|detail| ProtocolError::new(FailureKind::Cloud, detail))
                })?;
            let mut conversation = Vec::with_capacity(self.history.len() + 1);
            conversation.push(CloudTurn::User(task_message.clone()));
            conversation.extend(self.history.iter().cloned());
            let request = CloudRequest {
                instructions: pseudonymiser.template(CLOUD_PROMPT),
                conversation,
                reply_schema: schema,
                max_output_tokens: fan_out.cloud.config().max_output_tokens(),
            };
            let reply = fan_out
                .cloud
                .send(budget, request)
                .await
                .map_err(|error| ProtocolError::new(FailureKind::Cloud, error.to_string()))?;
            self.cloud_requests = request_number;
            let step = cloud_step(&reply.object)
                .ok_or_else(|| invalid("the reply's parts do not match the request's schema"))?;
            check_step(&step)?;
            let text = self
                .mapping
                .restore(&step.text)
                .map_err(|error| invalid(error.to_string()))?;
            match step.step {
                Step::Answer => {
                    check_citations(&step, &self.citations)?;
                    let citations = step
                        .citations
                        .iter()
                        .filter_map(|name| self.citations.find(name))
                        .map(|cited| CitedPassage {
                            citation: format!("C{}", cited.number),
                            document: task.documents[cited.document].name.clone(),
                            range: cited.range.clone(),
                            text: cited.text.clone(),
                        })
                        .collect();
                    return Ok(FanOutOutcome::Answer { text, citations });
                }
                Step::NotFound => return Ok(FanOutOutcome::NotFound { text }),
                Step::Plan if last => return Err(invalid("the last request planned again")),
                Step::Plan => {}
            }
            let jobs = plan_jobs(
                &step,
                &self.mapping,
                pseudonymiser,
                material,
                longest_chunk,
                fan_out.local,
                task.limits.local_output_tokens,
            )?;
            let report = self
                .run_plan(fan_out, task, pseudonymiser, material, jobs, request_number)
                .await?;
            self.history.push(CloudTurn::Assistant(reply.turn));
            self.history.push(CloudTurn::User(report));
        }
        Err(invalid("the requests ended without an answer"))
    }

    /// Runs, filters and logs one plan.
    async fn run_plan(
        &mut self,
        fan_out: &FanOut<'_>,
        task: &FanOutTask,
        pseudonymiser: &Pseudonymiser<'_>,
        material: &Material,
        jobs: Vec<PlannedJob>,
        round: u32,
    ) -> Result<OutboundText, ProtocolError> {
        let needed: u64 = jobs.iter().map(|job| job.chunks.len() as u64).sum();
        if needed > self.local_requests_left {
            return Err(ProtocolError::new(
                FailureKind::LocalLimit,
                "the plan needs more local requests than are left",
            ));
        }
        let mut requests = Vec::new();
        let mut slots = Vec::new();
        for (job_position, job) in jobs.iter().enumerate() {
            for chunk_position in &job.chunks {
                let chunk = &material.chunks[*chunk_position];
                let text = &task.documents[chunk.document].text[chunk.range.clone()];
                requests.push(
                    local_request(text, &job.question, task.limits.local_output_tokens)
                        .map_err(|detail| ProtocolError::new(FailureKind::Local, detail))?,
                );
                slots.push(Slot {
                    job: job_position,
                    chunk: *chunk_position,
                    started: false,
                    result: None,
                });
            }
        }
        self.local_requests_left -= needed;
        self.current = Some(CurrentRound {
            round,
            jobs,
            slots: Mutex::new(slots),
        });
        let concurrency =
            usize::try_from(fan_out.local.config().max_concurrent_requests()).unwrap_or(1);
        if let Some(current) = &self.current {
            run_round(fan_out.local, requests, &current.slots, concurrency).await?;
        }
        let Some(current) = &self.current else {
            return Err(ProtocolError::new(FailureKind::Local, "the round was lost"));
        };
        let (report, records, tokens) = {
            let slots = current
                .slots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let tokens = slots
                .iter()
                .filter_map(|slot| match &slot.result {
                    Some(Ok(answer)) => Some(answer.prompt_tokens + answer.completion_tokens),
                    _ => None,
                })
                .sum::<u64>();
            let (reports, outcomes) = filter_round(
                &slots,
                &current.jobs,
                material,
                &task.documents,
                pseudonymiser,
                FilterState {
                    mapping: &mut self.mapping,
                    citations: &mut self.citations,
                    verified_texts: &mut self.verified_texts,
                    sent_bytes: &mut self.sent_bytes,
                },
            )?;
            let report = messages::report(
                pseudonymiser,
                round,
                self.local_requests_left as u32,
                reports,
            )
            .map_err(|error| {
                ProtocolError::new(FailureKind::Pseudonymisation, error.to_string())
            })?;
            let records = round_records(task, current, &slots, material, Some(&outcomes), None)?;
            (report, records, tokens)
        };
        self.local_tokens += tokens;
        self.current = None;
        for record in records {
            append(fan_out.transcript, record)?;
        }
        Ok(report)
    }

    /// Records a round the task ended during.
    pub(crate) fn record_unfinished_round(
        &mut self,
        transcript: &Transcript,
        task: &FanOutTask,
        material: &Material,
        kind: FailureKind,
    ) -> Result<(), ProtocolError> {
        let Some(current) = self.current.take() else {
            return Ok(());
        };
        let records = {
            let slots = current
                .slots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            round_records(task, &current, &slots, material, None, Some(kind))?
        };
        for record in records {
            append(transcript, record)?;
        }
        Ok(())
    }
}

/// One transcript line per job of a round. Without outcomes, finished results are not
/// handled and started requests without a result carry `kind`.
fn round_records(
    task: &FanOutTask,
    current: &CurrentRound,
    slots: &[Slot],
    material: &Material,
    outcomes: Option<&[ChunkOutcome]>,
    kind: Option<FailureKind>,
) -> Result<Vec<TaskRecord>, ProtocolError> {
    let mut records = Vec::new();
    for (job_position, job) in current.jobs.iter().enumerate() {
        let mut chunks = Vec::new();
        for (index, slot) in slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.job == job_position)
        {
            if !slot.started && slot.result.is_none() {
                continue;
            }
            let chunk = &material.chunks[slot.chunk];
            let document_id = format!("D{}", chunk.document + 1);
            let outcome = outcomes.and_then(|outcomes| outcomes.get(index));
            let (answer, digest, tokens, error_kind) = match &slot.result {
                Some(Ok(answer)) => (
                    Some(answer.object.clone()),
                    Some(answer.model.digest.clone()),
                    Some((answer.prompt_tokens, answer.completion_tokens)),
                    None,
                ),
                Some(Err(error)) => (None, None, None, Some(error.to_string())),
                None => (None, None, None, kind.map(|kind| kind.name().to_string())),
            };
            let outcome_name = match (outcome, &slot.result) {
                (Some(outcome), _) => outcome.outcome,
                (None, Some(Ok(_))) => "not_handled",
                (None, _) => "failed",
            };
            let quotes = outcome
                .map(|outcome| {
                    outcome
                        .quotes
                        .iter()
                        .map(|(verified, citation)| QuoteLine {
                            document: document_id.clone(),
                            verified: *verified,
                            citation: citation.map(|number| format!("C{number}")),
                        })
                        .collect()
                })
                .unwrap_or_default();
            chunks.push(ChunkRunLine {
                document: chunk.document as u32 + 1,
                chunk: chunk.number as u32,
                local_model_digest: digest,
                answer: answer.map(|object| {
                    serde_json::Value::Object(
                        object
                            .iter()
                            .map(|(key, value)| (key.clone(), value.to_serde_json()))
                            .collect(),
                    )
                }),
                error_kind,
                quotes,
                outcome: outcome_name.to_string(),
                prompt_tokens: tokens.map(|(prompt, _)| prompt),
                completion_tokens: tokens.map(|(_, completion)| completion),
            });
        }
        let runs = JobRuns {
            round: current.round,
            job: job_position as u32 + 1,
            question: job.question.clone(),
            terms: job.terms.clone(),
            chunks,
        };
        records.push(job_record(&task.task_id, runs)?);
    }
    Ok(records)
}
