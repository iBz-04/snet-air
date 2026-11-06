use std::time::Duration;

use snet_cloud::{
    Charge, CloudClient, CloudRequest, CloudSchema, CloudTurn, Destination, JsonValue,
    OutboundText, TaskBudget, TaskLimits,
};
use snet_local::{LocalAnswer, LocalModel};
use snet_protocol::{
    Ending, FailureKind, ProtocolError, append, ended, holds_bracket, question_is_valid, refused,
};
use snet_pseudonym::{OutboundJson, Policy, Pseudonymiser, Restored, TaskMapping};
use snet_transcript::Transcript;

use crate::log;
use crate::prompts::CLOUD_PROMPT;
use crate::screen::{self, Screened};

/// How many earlier exchanges of a session go to the local model with a
/// new question.
pub const MAX_EARLIER_EXCHANGES: usize = 8;
const MAX_ANSWER_CHARACTERS: usize = 32_000;
const SECONDS: std::ops::RangeInclusive<u64> = 1..=3_600;
const MIN_LOCAL_OUTPUT_TOKENS: u64 = 64;
const SCHEMA_NAME: &str = "relay_answer";

/// A question asked earlier in the session and the answer it got, both as
/// the person saw them. They stay on the machine: the local model folds
/// what is needed into the next cloud question.
#[derive(Clone)]
pub struct EarlierExchange {
    pub question: String,
    pub answer: String,
}

/// A person's question without files.
#[derive(Clone)]
pub struct RelayTask {
    pub task_id: String,
    pub question: String,
    pub earlier: Vec<EarlierExchange>,
    pub seconds: u64,
    pub local_output_tokens: u64,
}

impl std::fmt::Debug for RelayTask {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RelayTask")
            .field("task_id", &self.task_id)
            .field("question_characters", &self.question.chars().count())
            .field("earlier", &self.earlier.len())
            .field("seconds", &self.seconds)
            .finish()
    }
}

/// Which model answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// The local model answered; nothing left the machine.
    Local,
    /// The cloud model answered this question, exactly as it read it,
    /// pseudonyms included, so the person can see what left the machine.
    Cloud { sent_question: String },
}

#[derive(Debug)]
pub struct RelayAnswer {
    pub task_id: String,
    pub text: Restored,
    pub route: Route,
    pub charge: Charge,
    pub local_tokens: u64,
    pub task_end_line: u64,
}

/// Runs relay tasks over one local model, one cloud endpoint, one
/// pseudonymisation policy and one transcript.
pub struct Relay<'a> {
    pub cloud: &'a CloudClient<Transcript>,
    pub local: &'a LocalModel,
    pub policy: &'a Policy,
    pub transcript: &'a Transcript,
    /// Never wider than the endpoint's task limits.
    pub budget: TaskLimits,
}

/// What the cloud step gives back on success.
struct Answered {
    text: Restored,
    route: Route,
}

impl Relay<'_> {
    pub async fn run(&self, task: &RelayTask) -> Result<RelayAnswer, ProtocolError> {
        let config = self.cloud.config();
        let budget = match TaskBudget::new(&task.task_id, self.budget) {
            Ok(budget) => budget,
            Err(_) => return Err(self.refuse(None, "task_id")),
        };
        if !self.budget.within(&config.task_limits()) {
            return Err(self.refuse(Some(&task.task_id), "budget"));
        }
        if let Some(field) = invalid_field(task, self.local.config().max_output_tokens()) {
            return Err(self.refuse(Some(&task.task_id), &field));
        }
        let local_request = screen::request(task)?;
        if self.local.check(&local_request).is_err() {
            return Err(self.refuse(Some(&task.task_id), "question"));
        }
        append(
            self.transcript,
            log::started(self.cloud, self.local, self.policy.digest(), task)?,
        )?;

        let mut local_tokens = 0;
        let limit = Duration::from_secs(task.seconds);
        let result = match tokio::time::timeout(
            limit,
            self.screen_then_ask(task, &budget, local_request, &mut local_tokens),
        )
        .await
        {
            Ok(result) => result,
            Err(_elapsed) => Err(ProtocolError::new(
                FailureKind::Timeout,
                "the task's time limit passed",
            )),
        };
        self.finish(task, &budget, local_tokens, result)
    }

    async fn screen_then_ask(
        &self,
        task: &RelayTask,
        budget: &TaskBudget,
        local_request: snet_local::LocalRequest,
        local_tokens: &mut u64,
    ) -> Result<Answered, ProtocolError> {
        let (_screening, screened) = self
            .local_step(
                task,
                1,
                &task.question,
                &local_request,
                local_tokens,
                screen::screened,
            )
            .await?;
        let (cloud_question, private_terms) = match screened {
            Screened::Local { answer } => {
                // An empty mapping restores text without placeholders as it
                // is, and refuses one that holds a placeholder.
                let text = TaskMapping::new().restore(&answer).map_err(|error| {
                    ProtocolError::new(FailureKind::InvalidLocalReply, error.to_string())
                })?;
                return Ok(Answered {
                    text,
                    route: Route::Local,
                });
            }
            Screened::Cloud {
                cloud_question,
                private_terms,
            } => (cloud_question, private_terms),
        };
        // A second pass over the question about to leave: the local model
        // lists any private detail its first pass kept, so privacy does not
        // rest on one reply of a small model.
        let check_request = screen::check_request(&cloud_question, task.local_output_tokens)?;
        let (_check, missed) = self
            .local_step(
                task,
                2,
                &cloud_question,
                &check_request,
                local_tokens,
                screen::private_terms,
            )
            .await?;
        let mut terms = private_terms;
        terms.extend(missed);

        let task_policy = self.policy.with_task_terms(&terms).map_err(|error| {
            ProtocolError::new(FailureKind::Pseudonymisation, error.to_string())
        })?;
        let config = self.cloud.config();
        let destination = Destination::new(config.name(), config.base_url().host());
        let pseudonymiser = task_policy.for_destination(destination);
        let mut mapping = TaskMapping::new();
        let question = pseudonymiser
            .text(&mut mapping, &cloud_question)
            .map_err(|error| {
                ProtocolError::new(FailureKind::Pseudonymisation, error.to_string())
            })?;
        let sent_question = question.as_str().to_string();
        let message = pseudonymiser
            .typed(&OutboundJson::Object(vec![(
                "question",
                OutboundJson::Text(question),
            )]))
            .map_err(|error| {
                ProtocolError::new(FailureKind::Pseudonymisation, error.to_string())
            })?;
        let text = self
            .ask_cloud(budget, &pseudonymiser, &mapping, message)
            .await?;
        Ok(Answered {
            text,
            route: Route::Cloud { sent_question },
        })
    }

    /// Asks the local model, records the step, and checks its reply with
    /// `check`; local tokens are added to `local_tokens`.
    async fn local_step<T>(
        &self,
        task: &RelayTask,
        step: u32,
        question: &str,
        request: &snet_local::LocalRequest,
        local_tokens: &mut u64,
        check: impl Fn(&snet_local::JsonObject) -> Result<T, ProtocolError>,
    ) -> Result<(LocalAnswer, T), ProtocolError> {
        let checked = match self.local.ask(request).await {
            Ok(answer) => {
                *local_tokens += answer.prompt_tokens + answer.completion_tokens;
                check(&answer.object).map(|value| (answer, value))
            }
            Err(error) => Err(ProtocolError::new(FailureKind::Local, error.to_string())),
        };
        let record = match &checked {
            Ok((answer, _)) => log::exchange(&task.task_id, step, question, Some(answer), None)?,
            Err(error) => log::exchange(&task.task_id, step, question, None, Some(error.kind))?,
        };
        append(self.transcript, record)?;
        checked
    }

    async fn ask_cloud(
        &self,
        budget: &TaskBudget,
        pseudonymiser: &Pseudonymiser<'_>,
        mapping: &TaskMapping,
        message: OutboundText,
    ) -> Result<Restored, ProtocolError> {
        let schema = pseudonymiser
            .typed(&OutboundJson::Object(vec![
                ("type", OutboundJson::Static("object")),
                (
                    "properties",
                    OutboundJson::Object(vec![(
                        "answer",
                        OutboundJson::Object(vec![("type", OutboundJson::Static("string"))]),
                    )]),
                ),
                (
                    "required",
                    OutboundJson::Array(vec![OutboundJson::Static("answer")]),
                ),
                ("additionalProperties", OutboundJson::Bool(false)),
            ]))
            .map_err(|error| ProtocolError::new(FailureKind::Pseudonymisation, error.to_string()))
            .and_then(|text| {
                CloudSchema::new(SCHEMA_NAME, text)
                    .map_err(|error| ProtocolError::new(FailureKind::Cloud, error.to_string()))
            })?;
        let request = CloudRequest {
            instructions: pseudonymiser.template(CLOUD_PROMPT),
            conversation: vec![CloudTurn::User(message)],
            reply_schema: schema,
            max_output_tokens: self.cloud.config().max_output_tokens(),
        };
        let reply = self
            .cloud
            .send(budget, request)
            .await
            .map_err(|error| ProtocolError::new(FailureKind::Cloud, error.to_string()))?;
        let Some(JsonValue::String(answer)) = reply.object.get("answer") else {
            return Err(invalid_reply("the reply has no string `answer`"));
        };
        let characters = answer.chars().count();
        if answer.trim().is_empty() || characters > MAX_ANSWER_CHARACTERS {
            return Err(invalid_reply(
                "the answer is empty or over 32 000 characters",
            ));
        }
        mapping
            .restore(answer)
            .map_err(|error| invalid_reply(error.to_string()))
    }

    /// Records a refused task and returns its error; a failure to record
    /// is returned instead.
    fn refuse(&self, task_id: Option<&str>, field: &str) -> ProtocolError {
        let recorded = refused(task_id, field).and_then(|record| append(self.transcript, record));
        match recorded {
            Ok(_) => ProtocolError::new(
                FailureKind::Input,
                format!("the field `{field}` is invalid"),
            ),
            Err(error) => error,
        }
    }

    fn finish(
        &self,
        task: &RelayTask,
        budget: &TaskBudget,
        local_tokens: u64,
        result: Result<Answered, ProtocolError>,
    ) -> Result<RelayAnswer, ProtocolError> {
        if let Err(error) = &result
            && error.kind == FailureKind::Record
        {
            return Err(error.clone());
        }
        let charge = budget
            .charged()
            .map_err(|error| ProtocolError::new(FailureKind::Cloud, error.to_string()))?;
        let (outcome, answer, error_kind) = match &result {
            Ok(answered) => ("answer", Some(answered.text.as_str()), None),
            Err(error) => ("failed", None, Some(error.kind)),
        };
        let task_end_line = append(
            self.transcript,
            ended(
                &task.task_id,
                Ending {
                    outcome,
                    error_kind,
                    answer,
                    citations: &[],
                    charge,
                    local_tokens,
                },
            )?,
        )?;
        let answered = result?;
        Ok(RelayAnswer {
            task_id: task.task_id.clone(),
            text: answered.text,
            route: answered.route,
            charge,
            local_tokens,
            task_end_line,
        })
    }
}

/// Runs one screening pass over a greeting and drops its answer, so the
/// model is loaded and its runtime holds the instructions in its prompt
/// cache before the person's first question.
pub async fn prime(local: &LocalModel, local_output_tokens: u64) -> Result<(), ProtocolError> {
    let task = RelayTask {
        task_id: "prime".to_string(),
        question: "hi".to_string(),
        earlier: Vec::new(),
        seconds: 60,
        local_output_tokens,
    };
    let request = screen::request(&task)?;
    local
        .ask(&request)
        .await
        .map(|_answer| ())
        .map_err(|error| ProtocolError::new(FailureKind::Local, error.to_string()))
}

fn invalid_reply(detail: impl Into<String>) -> ProtocolError {
    ProtocolError::new(FailureKind::InvalidCloudReply, detail)
}

/// The first field that breaks the input checks.
fn invalid_field(task: &RelayTask, local_output_cap: u64) -> Option<String> {
    if !question_is_valid(&task.question) {
        return Some("question".to_string());
    }
    if task.earlier.len() > MAX_EARLIER_EXCHANGES {
        return Some("earlier".to_string());
    }
    for (index, exchange) in task.earlier.iter().enumerate() {
        if !question_is_valid(&exchange.question) {
            return Some(format!("earlier[{index}].question"));
        }
        if exchange.answer.trim().is_empty()
            || exchange.answer.chars().count() > MAX_ANSWER_CHARACTERS
            || holds_bracket(&exchange.answer)
        {
            return Some(format!("earlier[{index}].answer"));
        }
    }
    if !SECONDS.contains(&task.seconds) {
        return Some("seconds".to_string());
    }
    if !(MIN_LOCAL_OUTPUT_TOKENS..=local_output_cap).contains(&task.local_output_tokens) {
        return Some("local_output_tokens".to_string());
    }
    None
}
