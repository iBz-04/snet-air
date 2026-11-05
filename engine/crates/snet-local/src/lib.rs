//! The local model, served by Ollama on this machine. It is the only model
//! that reads the user's material, so it is reached over loopback only.

mod client;
mod config;
mod errors;
mod exchange;
mod reply;
mod request;

pub use client::LocalModel;
pub use config::{DEFAULT_OLLAMA_ADDRESS, LocalBounds, LocalConfig};
pub use errors::{ConfigError, LocalError};
pub use reply::{ListedModel, LocalAnswer};
pub use request::LocalRequest;
pub use snet_wire::{ConversationMessage, ConversationRole, JsonObject, JsonValue, ReplySchema};
