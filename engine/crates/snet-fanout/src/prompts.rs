use sha2::{Digest, Sha256};

/// The cloud model's instructions.
pub const CLOUD_PROMPT: &str = include_str!("../prompts/cloud.txt");
/// The local model's instructions.
pub const LOCAL_PROMPT: &str = include_str!("../prompts/local.txt");

pub(crate) fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
