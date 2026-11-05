use std::io::Read;
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::errors::PolicyError;
use crate::text::{fold, nfc};

pub(crate) const MAX_TERM_CHARACTERS: usize = 200;
pub(crate) const MIN_TERM_CHARACTERS: usize = 2;
/// A dictionary over this is refused before more of it is read.
const MAX_DICTIONARY_BYTES: u64 = 16 * 1024 * 1024;

/// One dictionary as read: its file bytes for the digest and its folded,
/// distinct terms.
pub(crate) struct DictionaryFile {
    pub(crate) bytes: Zeroizing<Vec<u8>>,
    pub(crate) terms: Vec<Zeroizing<String>>,
}

pub(crate) fn read(path_text: &str, field: &str) -> Result<DictionaryFile, PolicyError> {
    let path = Path::new(path_text);
    if !path.is_absolute() {
        return Err(crate::policy::file::invalid(
            field,
            "must be an absolute path",
        ));
    }
    let refuse = |reason: String| PolicyError::Dictionary {
        path: PathBuf::from(path),
        reason,
    };
    let file = std::fs::File::open(path).map_err(|error| refuse(error.to_string()))?;
    let length = file
        .metadata()
        .map_err(|error| refuse(error.to_string()))?
        .len();
    if length > MAX_DICTIONARY_BYTES {
        return Err(refuse("it is larger than 16 MiB".to_string()));
    }
    let capacity = usize::try_from(length).map_err(|_| refuse("it is too large".to_string()))?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(capacity));
    file.take(MAX_DICTIONARY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| refuse(error.to_string()))?;
    if bytes.len() as u64 > MAX_DICTIONARY_BYTES {
        return Err(refuse("it is larger than 16 MiB".to_string()));
    }
    let terms = terms(path, &bytes)?;
    Ok(DictionaryFile { bytes, terms })
}

pub(crate) fn terms(path: &Path, bytes: &[u8]) -> Result<Vec<Zeroizing<String>>, PolicyError> {
    let refuse = |line: usize, reason: &'static str| PolicyError::DictionaryLine {
        path: PathBuf::from(path),
        line,
        reason,
    };
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(refuse(1, "the file starts with a byte-order mark"));
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            let line = bytes[..error.valid_up_to()]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count()
                + 1;
            return Err(refuse(line, "the line is not UTF-8"));
        }
    };
    let body = text.strip_suffix('\n').unwrap_or(text);
    let mut terms: Vec<Zeroizing<String>> = Vec::new();
    if text.is_empty() {
        return Ok(terms);
    }
    for (index, raw_line) in body.split('\n').enumerate() {
        let line = index + 1;
        let written = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if written.is_empty() {
            return Err(refuse(line, "the line is empty"));
        }
        // The checks below read the NFC form, so a term saved decomposed
        // (a base letter and a combining accent) is judged as it matches.
        let term = nfc(written);
        let characters = term.chars().count();
        if !(MIN_TERM_CHARACTERS..=MAX_TERM_CHARACTERS).contains(&characters) {
            return Err(refuse(line, "a term must be 2 to 200 characters"));
        }
        let alphanumeric = |character: Option<char>| {
            character.is_some_and(|character| character.is_alphabetic() || character.is_numeric())
        };
        if !alphanumeric(term.chars().next()) || !alphanumeric(term.chars().next_back()) {
            return Err(refuse(
                line,
                "a term must start and end with a letter or digit",
            ));
        }
        terms.push(fold(&term));
    }
    terms.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
    terms.dedup_by(|left, right| left.as_str() == right.as_str());
    Ok(terms)
}
