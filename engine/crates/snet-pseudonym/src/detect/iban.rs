use iban::Iban;
use zeroize::Zeroizing;

use super::untouched;
use crate::text::character_after;

const MAX_IBAN_CHARACTERS: usize = 34;
const GROUP: usize = 4;

fn is_iban_character(character: char) -> bool {
    character.is_ascii_uppercase() || character.is_ascii_digit()
}

fn starts_like_an_iban(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 4
        && bytes[0].is_ascii_uppercase()
        && bytes[1].is_ascii_uppercase()
        && bytes[2].is_ascii_digit()
        && bytes[3].is_ascii_digit()
}

fn validates(joined: &str) -> bool {
    starts_like_an_iban(joined) && joined.parse::<Iban>().is_ok()
}

/// An IBAN at one start, joined or in groups of four, trying shorter group
/// counts when the longest fails.
pub(super) fn candidate_at(text: &str, start: usize) -> Option<usize> {
    if !character_after(text, start).is_some_and(|character| character.is_ascii_uppercase()) {
        return None;
    }
    let first_end = super::run_end(text, start, MAX_IBAN_CHARACTERS, is_iban_character)?;
    let first_length = first_end - start;
    if first_length > GROUP {
        let joined = &text[start..first_end];
        return (untouched(text, start, first_end) && validates(joined)).then_some(first_end);
    }
    if first_length != GROUP {
        return None;
    }
    let mut group_ends = vec![first_end];
    let mut position = first_end;
    let mut total = GROUP;
    while character_after(text, position) == Some(' ') {
        let group_start = position + 1;
        let Some(group_end) = super::run_end(text, group_start, GROUP, is_iban_character) else {
            break;
        };
        let length = group_end - group_start;
        if length == 0 || total + length > MAX_IBAN_CHARACTERS {
            break;
        }
        total += length;
        group_ends.push(group_end);
        position = group_end;
        if length < GROUP {
            break;
        }
    }
    for count in (2..=group_ends.len()).rev() {
        let end = group_ends[count - 1];
        if !untouched(text, start, end) {
            continue;
        }
        let printed = &text[start..end];
        let mut joined = Zeroizing::new(String::with_capacity(printed.len()));
        joined.extend(printed.chars().filter(|character| *character != ' '));
        if validates(&joined) {
            return Some(end);
        }
    }
    None
}
