//! The fan-out protocol: the cloud model plans small jobs, the local model
//! runs them on every matching chunk of large material, and only checked,
//! pseudonymised findings leave.

mod chunks;
mod fanout;
mod filter;
mod log;
mod messages;
mod plan;
mod prompts;
mod rounds;
mod run;
mod step;
mod task;

pub use fanout::{FanOut, FanOutAnswer, FanOutOutcome};
pub use prompts::{CLOUD_PROMPT, LOCAL_PROMPT};
pub use snet_protocol::{CitedPassage, Document, FailureKind, ProtocolError};
pub use task::{FanOutLimits, FanOutTask};
