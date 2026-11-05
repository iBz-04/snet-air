use std::collections::BTreeMap;
use std::ops::Range;

use snet_wire::JsonValue;
use zeroize::Zeroizing;

use crate::errors::PseudonymError;
use crate::json_write::{self, Counter, Sink};
use crate::mapping::TaskMapping;
use crate::text::{TOKEN_CLOSE, TOKEN_OPEN, character_position, is_bracket};

const MAX_TOKEN_CHARACTERS: usize = 32 + 1 + 10;
const MAX_JSON_DEPTH: usize = 64;
const MAX_JSON_BYTES: usize = 1024 * 1024;
/// Characters a restored value may touch without a separator.
const SAFE: &[char] = &[
    '(', ')', '[', ']', '{', '}', '<', '>', '"', ';', '!', '?', '\'',
];

/// Restored text with the byte ranges of its restored values. It
/// holds real values, so it lives in a wiped buffer and its `Debug` shows
/// only sizes.
pub struct Restored {
    text: Zeroizing<String>,
    ranges: Vec<Range<usize>>,
}

impl std::fmt::Debug for Restored {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Restored")
            .field("bytes", &self.text.len())
            .field("values", &self.ranges.len())
            .finish()
    }
}

impl Restored {
    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }
}

/// Restored JSON as serialised text in a wiped buffer.
pub struct RestoredJson {
    text: Zeroizing<String>,
}

impl std::fmt::Debug for RestoredJson {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RestoredJson")
            .field("bytes", &self.text.len())
            .finish()
    }
}

impl RestoredJson {
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

enum Piece<'t> {
    Literal(&'t str),
    Value(&'t str),
}

impl TaskMapping {
    /// Replaces each token of this task with its first surface form,
    /// separating restored values from neighbours their detectors would
    /// refuse.
    pub fn restore(&self, text: &str) -> Result<Restored, PseudonymError> {
        let pieces = self.pieces(text)?;
        let mut counter = Counter(0);
        emit(&pieces, &mut counter, &mut Vec::new());
        let mut output = Zeroizing::new(String::with_capacity(counter.0));
        let mut ranges = Vec::new();
        emit(&pieces, &mut *output, &mut ranges);
        Ok(Restored {
            text: output,
            ranges,
        })
    }

    /// Restores every decoded string and key of `value` and serialises the
    /// result.
    pub fn restore_json(&self, value: &JsonValue) -> Result<RestoredJson, PseudonymError> {
        if json_write::nests_deeper_than(value, MAX_JSON_DEPTH) {
            return Err(PseudonymError::JsonTooDeep);
        }
        let mut size = Counter(0);
        json_write::value(&mut size, value);
        if size.0 > MAX_JSON_BYTES {
            return Err(PseudonymError::JsonTooLarge { size: size.0 });
        }
        let restored = self.restore_value(value, "")?;
        let mut counter = Counter(0);
        restored.write(&mut counter);
        let mut text = Zeroizing::new(String::with_capacity(counter.0));
        restored.write(&mut *text);
        Ok(RestoredJson { text })
    }

    fn restore_value(
        &self,
        value: &JsonValue,
        path: &str,
    ) -> Result<RestoredValue, PseudonymError> {
        Ok(match value {
            JsonValue::String(text) => RestoredValue::String(self.restore(text)?),
            JsonValue::Array(items) => {
                let mut restored = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    restored.push(self.restore_value(item, &format!("{path}/{index}"))?);
                }
                RestoredValue::Array(restored)
            }
            JsonValue::Object(members) => {
                let mut restored: BTreeMap<RestoredKey, RestoredValue> = BTreeMap::new();
                for (index, (key, item)) in members.iter().enumerate() {
                    let key = RestoredKey(self.restore(key)?);
                    let item = self.restore_value(item, &format!("{path}/{index}"))?;
                    if restored.insert(key, item).is_some() {
                        return Err(PseudonymError::KeysCollide {
                            path: if path.is_empty() {
                                "/".to_string()
                            } else {
                                path.to_string()
                            },
                        });
                    }
                }
                RestoredValue::Object(restored)
            }
            other => RestoredValue::Plain(other.clone()),
        })
    }

    fn pieces<'t>(&'t self, text: &'t str) -> Result<Vec<Piece<'t>>, PseudonymError> {
        let mut pieces = Vec::new();
        let mut copied = 0;
        let mut characters = text.char_indices();
        while let Some((index, character)) = characters.next() {
            if !is_bracket(character) {
                continue;
            }
            let position = character_position(text, index);
            if character != TOKEN_OPEN {
                return Err(PseudonymError::StrayBracket { position });
            }
            let inner_start = index + character.len_utf8();
            let Some(close) = text[inner_start..]
                .char_indices()
                .take(MAX_TOKEN_CHARACTERS + 1)
                .find(|(_, inner)| is_bracket(*inner))
                .filter(|(_, inner)| *inner == TOKEN_CLOSE)
                .map(|(offset, _)| inner_start + offset)
            else {
                return Err(PseudonymError::TokenMalformed { position });
            };
            let (category, number) = parse_token(&text[inner_start..close])
                .ok_or(PseudonymError::TokenMalformed { position })?;
            let entry = self
                .find_token(category, number)
                .ok_or(PseudonymError::TokenUnknown { position })?;
            pieces.push(Piece::Literal(&text[copied..index]));
            pieces.push(Piece::Value(entry.surface.as_str()));
            copied = close + TOKEN_CLOSE.len_utf8();
            while characters.next().is_some_and(|(next, _)| next < close) {}
        }
        pieces.push(Piece::Literal(&text[copied..]));
        Ok(pieces)
    }
}

/// `NAME_N` with the name split at the last `_`: the name is a category
/// name, N a decimal number without leading zeros.
fn parse_token(inner: &str) -> Option<(&str, u32)> {
    let (name, number) = inner.rsplit_once('_')?;
    let well_formed = crate::policy::is_category_name(name)
        && !number.is_empty()
        && !number.starts_with('0')
        && number.chars().all(|character| character.is_ascii_digit());
    if !well_formed {
        return None;
    }
    Some((name, number.parse().ok()?))
}

fn is_safe(character: char, far_side: Option<char>) -> bool {
    if character.is_whitespace() || SAFE.contains(&character) {
        return true;
    }
    matches!(character, '.' | ',')
        && far_side.is_none_or(|far| far.is_whitespace() || SAFE.contains(&far))
}

/// The first two characters after piece `index`, across later pieces.
fn upcoming(pieces: &[Piece<'_>], index: usize) -> (Option<char>, Option<char>) {
    let mut characters = pieces[index + 1..].iter().flat_map(|piece| match piece {
        Piece::Literal(text) | Piece::Value(text) => text.chars(),
    });
    (characters.next(), characters.next())
}

fn push(
    sink: &mut dyn Sink,
    text: &str,
    written: &mut usize,
    last_two: &mut (Option<char>, Option<char>),
) {
    sink.push_str(text);
    *written += text.len();
    for character in text.chars() {
        *last_two = (last_two.1, Some(character));
    }
}

fn emit(pieces: &[Piece<'_>], sink: &mut dyn Sink, ranges: &mut Vec<Range<usize>>) {
    let mut written = 0usize;
    let mut last_two: (Option<char>, Option<char>) = (None, None);
    for (index, piece) in pieces.iter().enumerate() {
        match piece {
            Piece::Literal(text) => push(sink, text, &mut written, &mut last_two),
            Piece::Value(value) => {
                if let Some(before) = last_two.1
                    && !is_safe(before, last_two.0)
                {
                    push(sink, " ", &mut written, &mut last_two);
                }
                let start = written;
                push(sink, value, &mut written, &mut last_two);
                ranges.push(start..written);
                let (next, far) = upcoming(pieces, index);
                if let Some(next) = next
                    && !is_safe(next, far)
                {
                    let separator = if next.is_alphabetic() { "'" } else { " " };
                    push(sink, separator, &mut written, &mut last_two);
                }
            }
        }
    }
}

struct RestoredKey(Restored);

impl PartialEq for RestoredKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

impl Eq for RestoredKey {}

impl PartialOrd for RestoredKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RestoredKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.as_str().cmp(other.0.as_str())
    }
}

enum RestoredValue {
    Plain(JsonValue),
    String(Restored),
    Array(Vec<RestoredValue>),
    Object(BTreeMap<RestoredKey, RestoredValue>),
}

impl RestoredValue {
    fn write(&self, sink: &mut dyn Sink) {
        match self {
            RestoredValue::Plain(value) => json_write::value(sink, value),
            RestoredValue::String(text) => json_write::string(sink, text.as_str()),
            RestoredValue::Array(items) => {
                sink.push_str("[");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        sink.push_str(",");
                    }
                    item.write(sink);
                }
                sink.push_str("]");
            }
            RestoredValue::Object(members) => {
                sink.push_str("{");
                for (index, (key, item)) in members.iter().enumerate() {
                    if index > 0 {
                        sink.push_str(",");
                    }
                    json_write::string(sink, key.0.as_str());
                    sink.push_str(":");
                    item.write(sink);
                }
                sink.push_str("}");
            }
        }
    }
}

#[cfg(test)]
mod tests;
