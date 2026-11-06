//! One recorded request to a configured cloud model.
//!
//! No workspace crate may depend on this one until the record store spec
//! allows it.

// The shared pieces moved to snet-wire keep their
// paths inside this crate.
pub(crate) use snet_wire::{io, json, schema};

mod budget;
mod client;
mod config;
mod errors;
mod record;
mod request;
mod resolve;
mod secret;
mod switch;
#[cfg(test)]
mod test_support;
mod tls;
mod transport;
mod truncation;
mod wire;

pub use budget::{BudgetError, Charge, Prices, TaskBudget, TaskLimits};
pub use client::{CloudClient, CloudReply};
pub use config::{
    BaseUrl, CertificateReader, EndpointConfig, InstructionRole, KeyHeader, OpenaiChatSettings,
    OutputCapField, PresetChoice, Provider, WireFormat, WireSettings,
};
pub use errors::{CloudError, ConfigError, Failure};
pub use record::{
    BytesWritten, OutboundRecord, OutboundRecordId, OutboundRecorder, OutcomeRecord, RecordStage,
    RecorderError,
};
pub use request::{AcceptedTurn, CloudRequest, CloudSchema, CloudTurn};
pub use secret::ApiKey;
pub use snet_pseudonym::{Destination, OutboundText, Report};
pub use snet_wire::ConversationRole;
pub use snet_wire::{JsonObject, JsonValue};
pub use snet_wire::{ReplySchema, SchemaError};
pub use switch::{CloudSwitch, SwitchSource};
pub use truncation::{TruncatedReply, TruncationStop};
pub use wire::ReportedUsage;
