//! The local model: served by Ollama on this machine, or a GGUF file run in
//! this process. It is the only model that reads the user's material, so it
//! never runs anywhere else.

mod client;
mod config;
mod errors;
mod exchange;
#[cfg(feature = "gguf")]
mod gguf;
mod reply;
mod request;

pub use client::LocalModel;
pub use config::{Backend, DEFAULT_OLLAMA_ADDRESS, LocalBounds, LocalConfig};
pub use errors::{ConfigError, LocalError};
pub use reply::{ListedModel, LocalAnswer};
pub use request::LocalRequest;
pub use snet_wire::{ConversationMessage, ConversationRole, JsonObject, JsonValue, ReplySchema};
