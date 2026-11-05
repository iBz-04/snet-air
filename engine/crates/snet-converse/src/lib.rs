//! The conversation protocol: the cloud model questions the local model
//! over a few rounds about material it never sees.

mod conversation;
mod errors;
mod log;
mod messages;
mod prompts;
mod quotes;
mod round;
mod task;

pub use conversation::{CitedPassage, Conversation, ConversationAnswer, ConversationOutcome};
pub use errors::{ConversationError, FailureKind};
pub use prompts::{CLOUD_PROMPT, LOCAL_PROMPT};
pub use task::{ConversationLimits, ConversationTask, Document};
