//! What the conversation and fan-out protocols share:
//! documents and the shared input checks, citations, failure kinds, the
//! task-refused and task-end records, the copied-quote and spelled-value
//! checks, and the cloud-step schema builder.

mod checks;
mod citations;
mod documents;
mod errors;
mod records;
mod schema;

pub use checks::{spells_out_a_value, without_tokens};
pub use citations::{Citations, Cited, CitedPassage, quote_range};
pub use documents::{Document, holds_bracket, name_is_valid, question_is_valid};
pub use errors::{FailureKind, ProtocolError};
pub use records::{Ending, append, ended, hex, now_milliseconds, refused};
pub use schema::step_schema;
