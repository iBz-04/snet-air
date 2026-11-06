//! The relay protocol for a question without files: the local model reads
//! it, finds the private details and writes a short question that stands
//! on its own; only that question, pseudonymised, reaches the cloud model,
//! whose answer is restored on the machine.

mod log;
mod prompts;
mod relay;
mod screen;
mod task;

pub use prompts::{CLOUD_PROMPT, LOCAL_PROMPT};
pub use relay::{Relay, RelayAnswer};
pub use task::{EarlierExchange, MAX_EARLIER_EXCHANGES, RelayTask, Route, prime};
