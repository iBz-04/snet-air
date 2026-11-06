use sha2::{Digest, Sha256};

/// The cloud model's instructions.
pub const CLOUD_PROMPT: &str = include_str!("../prompts/cloud.txt");
/// The local model's instructions for screening and condensing.
pub const LOCAL_PROMPT: &str = include_str!("../prompts/local.txt");
/// The local model's instructions for checking the condensed question.
pub const CHECK_PROMPT: &str = include_str!("../prompts/check.txt");

pub(crate) fn digest(text: &str) -> String {
    snet_protocol::hex(&Sha256::digest(text.as_bytes()))
}
