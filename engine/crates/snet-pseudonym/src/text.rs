use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

/// The pseudonym brackets and their look-alikes, refused in input
/// and outside tokens in restored text.
pub(crate) const TOKEN_OPEN: char = '⟦';
pub(crate) const TOKEN_CLOSE: char = '⟧';
pub(crate) const LOOKALIKE_OPEN: char = '〚';
pub(crate) const LOOKALIKE_CLOSE: char = '〛';

pub(crate) fn is_bracket(character: char) -> bool {
    matches!(
        character,
        TOKEN_OPEN | TOKEN_CLOSE | LOOKALIKE_OPEN | LOOKALIKE_CLOSE
    )
}

/// Alphanumeric: Unicode Alphabetic or Numeric, or `_`.
pub(crate) fn is_word_character(character: char) -> bool {
    character.is_alphabetic() || character.is_numeric() || character == '_'
}

pub(crate) fn character_before(text: &str, index: usize) -> Option<char> {
    text[..index].chars().next_back()
}

pub(crate) fn character_after(text: &str, index: usize) -> Option<char> {
    text[index..].chars().next()
}

pub(crate) fn character_position(text: &str, byte_index: usize) -> usize {
    text[..byte_index].chars().count()
}

/// The NFC form, sized in a first pass so the wiped buffer never moves
///.
pub(crate) fn nfc(text: &str) -> Zeroizing<String> {
    let size: usize = text.nfc().map(char::len_utf8).sum();
    let mut normal = String::with_capacity(size);
    normal.extend(text.nfc());
    Zeroizing::new(normal)
}

/// `I`, `ı`, `İ` and `i` become `i`; every other character its
/// Unicode lower case.
fn fold_character(character: char) -> impl Iterator<Item = char> {
    let turkish_i = matches!(character, 'I' | 'ı' | 'İ' | 'i');
    let lower = character.to_lowercase();
    std::iter::once('i')
        .filter(move |_| turkish_i)
        .chain(lower.filter(move |_| !turkish_i))
}

pub(crate) fn fold(text: &str) -> Zeroizing<String> {
    let size: usize = text
        .chars()
        .flat_map(fold_character)
        .map(char::len_utf8)
        .sum();
    let mut folded = String::with_capacity(size);
    folded.extend(text.chars().flat_map(fold_character));
    Zeroizing::new(folded)
}

/// A folded copy of a text with, for every folded byte, where the
/// character that produced it starts and ends in the original.
pub(crate) struct Folded {
    pub(crate) text: Zeroizing<String>,
    pub(crate) origin_start: Vec<usize>,
    pub(crate) origin_end: Vec<usize>,
}

pub(crate) fn fold_with_origins(text: &str) -> Folded {
    let folded = fold(text);
    let mut origin_start = Vec::with_capacity(folded.len());
    let mut origin_end = Vec::with_capacity(folded.len());
    for (start, character) in text.char_indices() {
        let end = start + character.len_utf8();
        for produced in fold_character(character) {
            for _ in 0..produced.len_utf8() {
                origin_start.push(start);
                origin_end.push(end);
            }
        }
    }
    Folded {
        text: folded,
        origin_start,
        origin_end,
    }
}

/// Letters and digits only, folded.
pub(crate) fn letters_and_digits(text: &str) -> Zeroizing<String> {
    let kept = |character: &char| character.is_alphabetic() || character.is_numeric();
    let size: usize = text
        .chars()
        .filter(kept)
        .flat_map(fold_character)
        .map(char::len_utf8)
        .sum();
    let mut reduced = String::with_capacity(size);
    reduced.extend(text.chars().filter(kept).flat_map(fold_character));
    Zeroizing::new(reduced)
}

/// A text built into a buffer of exactly its final size, for values that
/// must not leave unwiped copies behind when a buffer grows.
pub(crate) fn exact(parts: &[&str]) -> Zeroizing<String> {
    let size = parts.iter().map(|part| part.len()).sum();
    let mut text = String::with_capacity(size);
    for part in parts {
        text.push_str(part);
    }
    Zeroizing::new(text)
}
