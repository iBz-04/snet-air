use std::collections::{BTreeSet, HashSet};

use sha2::{Digest, Sha256};

use crate::errors::RulesError;
use crate::words::for_each_word;

pub const COMPILED_LIST: &str = include_str!("../forbidden_terms.sha256");

const DIGEST_BYTES: usize = 32;

/// The list holds SHA-256 digests of lowercase words, never the words, so the
/// repository that carries it names nothing.
#[derive(Debug)]
pub struct ForbiddenTerms {
    digests: HashSet<[u8; DIGEST_BYTES]>,
}

impl ForbiddenTerms {
    pub fn compiled() -> Result<Self, RulesError> {
        Self::parse(COMPILED_LIST)
    }

    pub fn parse(list: &str) -> Result<Self, RulesError> {
        let body = list.strip_suffix('\n').unwrap_or(list);
        if body.is_empty() {
            return Err(RulesError::ForbiddenTermsEmpty);
        }

        let mut digests = HashSet::new();
        for (index, line) in body.split('\n').enumerate() {
            let digest = parse_lowercase_hex_digest(line)
                .ok_or(RulesError::ForbiddenTermMalformed { line: index + 1 })?;
            digests.insert(digest);
        }
        Ok(Self { digests })
    }

    pub fn len(&self) -> usize {
        self.digests.len()
    }

    pub fn is_empty(&self) -> bool {
        self.digests.is_empty()
    }

    pub fn is_listed(&self, lowercase_word: &[u8]) -> bool {
        let digest: [u8; DIGEST_BYTES] = Sha256::digest(lowercase_word).into();
        self.digests.contains(&digest)
    }

    pub fn contains_listed_word(&self, bytes: &[u8]) -> bool {
        let mut found = false;
        for_each_word(bytes, |word, _| found |= self.is_listed(word));
        found
    }

    pub fn lines_with_listed_words(&self, bytes: &[u8]) -> BTreeSet<usize> {
        let mut lines = BTreeSet::new();
        for_each_word(bytes, |word, line| {
            if self.is_listed(word) {
                lines.insert(line);
            }
        });
        lines
    }

    /// Prints a `/`-separated repository path with every component that holds
    /// a listed word replaced by `<listed:N>`, N being its 1-based position.
    pub fn display_path(&self, relative_path: &str) -> String {
        relative_path
            .split('/')
            .enumerate()
            .map(|(index, component)| {
                if self.contains_listed_word(component.as_bytes()) {
                    format!("<listed:{}>", index + 1)
                } else {
                    component.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("/")
    }
}

fn parse_lowercase_hex_digest(line: &str) -> Option<[u8; DIGEST_BYTES]> {
    let characters = line.as_bytes();
    if characters.len() != DIGEST_BYTES * 2 {
        return None;
    }

    let (pairs, _) = characters.as_chunks::<2>();
    let mut digest = [0u8; DIGEST_BYTES];
    for (byte, [high, low]) in digest.iter_mut().zip(pairs) {
        *byte = lowercase_hex_value(*high)? << 4 | lowercase_hex_value(*low)?;
    }
    Some(digest)
}

fn lowercase_hex_value(character: u8) -> Option<u8> {
    match character {
        b'0'..=b'9' => Some(character - b'0'),
        b'a'..=b'f' => Some(character - b'a' + 10),
        _ => None,
    }
}
