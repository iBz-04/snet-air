//! The pieces the cloud client and the local client share.

mod conversation;
pub mod io;
pub mod json;
pub mod schema;

pub use conversation::{ConversationMessage, ConversationRole};
pub use json::{JsonObject, JsonValue};
pub use schema::{ReplySchema, SchemaError, SchemaViolation};
