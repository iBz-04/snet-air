//! The direct protocol: a question with no local material, answered by the
//! cloud model alone after its personal data is pseudonymised.

mod direct;
mod log;
mod prompts;

pub use direct::{Direct, DirectAnswer, DirectTask, EarlierExchange, MAX_EARLIER_EXCHANGES};
pub use prompts::CLOUD_PROMPT;
