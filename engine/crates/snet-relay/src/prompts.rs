use sha2::{Digest, Sha256};

/// The cloud model's instructions.
pub const CLOUD_PROMPT: &str = include_str!("../prompts/cloud.txt");
/// The local model's instructions, one text for screening and for the
/// check pass so Ollama's cache of the prompt's start serves both.
pub const LOCAL_PROMPT: &str = include_str!("../prompts/local.txt");

pub(crate) fn digest(text: &str) -> String {
    snet_protocol::hex(&Sha256::digest(text.as_bytes()))
}
