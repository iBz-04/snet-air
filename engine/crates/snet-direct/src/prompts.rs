use sha2::{Digest, Sha256};

/// The cloud model's instructions.
pub const CLOUD_PROMPT: &str = include_str!("../prompts/cloud.txt");

pub(crate) fn digest(text: &str) -> String {
    snet_protocol::hex(&Sha256::digest(text.as_bytes()))
}
