//! Personal data replaced with per-task pseudonyms on this machine before any text
//! leaves, and restored in the cloud's answers.

mod detect;
mod errors;
mod json_write;
mod mapping;
mod matching;
mod outbound;
mod policy;
mod pseudonymise;
mod resolve;
mod restore;
mod text;

pub use detect::Builtin;
pub use errors::{PolicyError, PseudonymError};
pub use mapping::TaskMapping;
pub use matching::{fold_for_matching, on_word_boundaries};
pub use outbound::{Destination, OutboundJson, OutboundText, Report};
pub use policy::{BUILTIN_POLICY, PassthroughRule, Policy};
pub use pseudonymise::Pseudonymiser;
pub use restore::{Restored, RestoredJson};
pub use snet_wire::{JsonObject, JsonValue};

#[cfg(test)]
mod test_support;
