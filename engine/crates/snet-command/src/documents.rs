use std::io::Read;

use snet_protocol::Document;

use crate::exit::CommandError;

/// The conversation protocol sends every file whole to the local model.
pub const CONVERSATION_FILE_BYTES: u64 = 64 * 1024;
/// The fan-out protocol splits each file into chunks.
pub const FAN_OUT_FILE_BYTES: u64 = 1024 * 1024;

/// Reads one UTF-8 text file of at most `limit` bytes, named as given.
pub fn read(name: &str, limit: u64, fanout: bool) -> Result<Document, CommandError> {
    let file = std::fs::File::open(name)
        .map_err(|error| CommandError::usage(format!("file `{name}`: {error}")))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| CommandError::usage(format!("file `{name}`: {error}")))?;
    if bytes.len() as u64 > limit {
        let advice = if fanout {
            "split it into smaller files"
        } else {
            "use --fanout, which reads large files in chunks"
        };
        return Err(CommandError::usage(format!(
            "file `{name}` is longer than {limit} bytes; {advice}"
        )));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| CommandError::usage(format!("file `{name}` is not UTF-8 text")))?;
    Ok(Document {
        name: name.to_string(),
        text,
    })
}
