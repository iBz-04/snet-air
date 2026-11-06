use std::time::Duration;

use snet_cloud::{
    Charge, CloudClient, CloudRequest, CloudSchema, CloudTurn, Destination, JsonValue,
    OutboundText, TaskBudget, TaskLimits,
};
use snet_protocol::{
    Ending, FailureKind, ProtocolError, append, ended, holds_bracket, question_is_valid, refused,
};
use snet_pseudonym::{OutboundJson, Policy, Pseudonymiser, Restored, TaskMapping};
use snet_transcript::Transcript;

use crate::log;
use crate::prompts::CLOUD_PROMPT;

/// How many earlier exchanges of a session go with a new question.
pub const MAX_EARLIER_EXCHANGES: usize = 8;
const MAX_ANSWER_CHARACTERS: usize = 32_000;
const SECONDS: std::ops::RangeInclusive<u64> = 1..=3_600;
const SCHEMA_NAME: &str = "direct_answer";

/// A question asked earlier in the session and the answer it got, both as
/// the person saw them.
#[derive(Clone)]
pub struct EarlierExchange {
    pub question: String,
    pub answer: String,
}

/// A person's question with no local material.
#[derive(Clone)]
pub struct DirectTask {
    pub task_id: String,
    pub question: String,
    pub earlier: Vec<EarlierExchange>,
    pub seconds: u64,
}

impl std::fmt::Debug for DirectTask {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DirectTask")
            .field("task_id", &self.task_id)
            .field("question_characters", &self.question.chars().count())
            .field("earlier", &self.earlier.len())
            .field("seconds", &self.seconds)
            .finish()
    }
}

#[derive(Debug)]
pub struct DirectAnswer {
    pub task_id: String,
    pub text: Restored,
    pub charge: Charge,
    pub task_end_line: u64,
}

/// Answers direct questions over one cloud endpoint, one pseudonymisation
/// policy and one transcript.
pub struct Direct<'a> {
    pub cloud: &'a CloudClient<Transcript>,
    pub policy: &'a Policy,
    pub transcript: &'a Transcript,
    /// Never wider than the endpoint's task limits.
    pub budget: TaskLimits,
}

impl Direct<'_> {
    pub async fn run(&self, task: &DirectTask) -> Result<DirectAnswer, ProtocolError> {
        let config = self.cloud.config();
        let budget = match TaskBudget::new(&task.task_id, self.budget) {
            Ok(budget) => budget,
            Err(_) => return Err(self.refuse(None, "task_id")),
        };
        if !self.budget.within(&config.task_limits()) {
            return Err(self.refuse(Some(&task.task_id), "budget"));
        }
        if let Some(field) = invalid_field(task) {
            return Err(self.refuse(Some(&task.task_id), &field));
        }

        let destination = Destination::new(config.name(), config.base_url().host());
        let pseudonymiser = self.policy.for_destination(destination);
        let mut mapping = TaskMapping::new();
        let message = match user_message(&pseudonymiser, &mut mapping, task) {
            Ok(message) => message,
            Err(error) => {
                append(self.transcript, refused(Some(&task.task_id), "question")?)?;
                return Err(error);
            }
        };
        append(
            self.transcript,
            log::started(self.cloud, self.policy.digest(), task)?,
        )?;

        let limit = Duration::from_secs(task.seconds);
        let result =
            match tokio::time::timeout(limit, self.ask(&budget, &pseudonymiser, &mapping, message))
                .await
            {
                Ok(result) => result,
                Err(_elapsed) => Err(ProtocolError::new(
                    FailureKind::Timeout,
                    "the task's time limit passed",
                )),
            };
        self.finish(task, &budget, result)
    }

    async fn ask(
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
        task: &DirectTask,
        budget: &TaskBudget,
        result: Result<Restored, ProtocolError>,
    ) -> Result<DirectAnswer, ProtocolError> {
        if let Err(error) = &result
            && error.kind == FailureKind::Record
        {
            return Err(error.clone());
        }
        let charge = budget
            .charged()
            .map_err(|error| ProtocolError::new(FailureKind::Cloud, error.to_string()))?;
        let (outcome, answer, error_kind) = match &result {
            Ok(text) => ("answer", Some(text.as_str()), None),
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
                    local_tokens: 0,
                },
            )?,
        )?;
        Ok(DirectAnswer {
            task_id: task.task_id.clone(),
            text: result?,
            charge,
            task_end_line,
        })
    }
}

fn invalid_reply(detail: impl Into<String>) -> ProtocolError {
    ProtocolError::new(FailureKind::InvalidCloudReply, detail)
}

/// The first field that breaks the input checks.
fn invalid_field(task: &DirectTask) -> Option<String> {
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
    None
}

/// The question and earlier exchanges as typed JSON, every text
/// pseudonymised with this task's mapping.
fn user_message(
    pseudonymiser: &Pseudonymiser<'_>,
    mapping: &mut TaskMapping,
    task: &DirectTask,
) -> Result<OutboundText, ProtocolError> {
    let pseudonymised = |error: snet_pseudonym::PseudonymError| {
        ProtocolError::new(FailureKind::Pseudonymisation, error.to_string())
    };
    let mut earlier = Vec::with_capacity(task.earlier.len());
    for exchange in &task.earlier {
        let question = pseudonymiser
            .text(mapping, &exchange.question)
            .map_err(pseudonymised)?;
        let answer = pseudonymiser
            .text(mapping, &exchange.answer)
            .map_err(pseudonymised)?;
        earlier.push(OutboundJson::Object(vec![
            ("question", OutboundJson::Text(question)),
            ("answer", OutboundJson::Text(answer)),
        ]));
    }
    let question = pseudonymiser
        .text(mapping, &task.question)
        .map_err(pseudonymised)?;
    pseudonymiser
        .typed(&OutboundJson::Object(vec![
            ("question", OutboundJson::Text(question)),
            ("earlier", OutboundJson::Array(earlier)),
        ]))
        .map_err(pseudonymised)
}
