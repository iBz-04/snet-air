use snet_pseudonym::{Destination, OutboundText, Report};
use snet_wire::{ConversationRole, JsonObject, ReplySchema, SchemaError};

use crate::config::EndpointConfig;
use crate::errors::Failure;

/// A reply this client accepted, kept as the text sent back to the same
/// endpoint as an assistant turn. Only an accepted reply makes
/// one, so restored text can never be sent back by mistake.
#[derive(Clone, PartialEq, Eq)]
pub struct AcceptedTurn {
    text: String,
    destination: Destination,
}

impl std::fmt::Debug for AcceptedTurn {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcceptedTurn")
            .field("bytes", &self.text.len())
            .field("destination", &self.destination)
            .finish()
    }
}

impl AcceptedTurn {
    /// The object serialised in `snet-wire`'s key order.
    pub(crate) fn accepted(object: &JsonObject, destination: Destination) -> Self {
        let text = serde_json::Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), value.to_serde_json()))
                .collect(),
        )
        .to_string();
        Self { text, destination }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn destination(&self) -> &Destination {
        &self.destination
    }
}

/// One conversation turn: text this engine wrote, or a reply the endpoint gave.
#[derive(Debug, Clone)]
pub enum CloudTurn {
    User(OutboundText),
    Assistant(AcceptedTurn),
}

impl CloudTurn {
    pub(crate) fn role(&self) -> ConversationRole {
        match self {
            CloudTurn::User(_) => ConversationRole::User,
            CloudTurn::Assistant(_) => ConversationRole::Assistant,
        }
    }

    pub(crate) fn text(&self) -> &str {
        match self {
            CloudTurn::User(text) => text.as_str(),
            CloudTurn::Assistant(turn) => turn.as_str(),
        }
    }
}

/// The reply schema as outbound text with a static name, parsed and checked
/// against the subset here.
#[derive(Debug, Clone)]
pub struct CloudSchema {
    text: OutboundText,
    checked: ReplySchema,
}

impl CloudSchema {
    pub fn new(name: &'static str, text: OutboundText) -> Result<Self, SchemaError> {
        let value: serde_json::Value =
            serde_json::from_str(text.as_str()).map_err(|_| SchemaError {
                path: "#".to_string(),
                reason: "the schema text is not JSON".to_string(),
            })?;
        let checked = ReplySchema::new(name, value)?;
        Ok(Self { text, checked })
    }

    pub fn checked(&self) -> &ReplySchema {
        &self.checked
    }
}

/// One structured request: the instructions, the conversation, the schema
/// the reply must follow and the most output tokens it may use. Every text
/// in it is pseudonymised text or an accepted reply.
#[derive(Debug, Clone)]
pub struct CloudRequest {
    pub instructions: OutboundText,
    pub conversation: Vec<CloudTurn>,
    pub reply_schema: CloudSchema,
    pub max_output_tokens: u64,
}

impl CloudRequest {
    /// The reports of the request's texts in order (instructions, user turns,
    /// schema), after checking that every text and reply belongs to the
    /// client's endpoint and every report names one policy.
    pub(crate) fn reports_for(&self, config: &EndpointConfig) -> Result<Vec<Report>, Failure> {
        let destination = Destination::new(config.name.clone(), config.base_url.host());
        let mut reports = vec![self.instructions.report().clone()];
        for turn in &self.conversation {
            match turn {
                CloudTurn::User(text) => reports.push(text.report().clone()),
                CloudTurn::Assistant(reply) => {
                    if reply.destination() != &destination {
                        return Err(Failure::DestinationMismatch);
                    }
                }
            }
        }
        reports.push(self.reply_schema.text.report().clone());
        if reports
            .iter()
            .any(|report| report.destination != destination)
        {
            return Err(Failure::DestinationMismatch);
        }
        if reports
            .iter()
            .any(|report| report.policy_digest != reports[0].policy_digest)
        {
            return Err(Failure::PolicyMismatch);
        }
        Ok(reports)
    }
}
