use super::{isolated_after, isolated_before};
use crate::text::character_after;

const MAX_DIGITS: usize = 15;
const MIN_DIGITS: usize = 8;
const NATIONAL_TRAILING_DIGITS: usize = 7;

pub(super) fn candidate_at(text: &str, start: usize) -> Option<usize> {
    international_at(text, start).or_else(|| national_at(text, start))
}

/// `+` or `00`, a digit from 1 to 9, then groups, ending after the
/// last whole group that keeps at most 15 digits, kept with 8 or more.
fn international_at(text: &str, start: usize) -> Option<usize> {
    let prefix = if text[start..].starts_with('+') {
        1
    } else if text[start..].starts_with("00") {
        2
    } else {
        return None;
    };
    if !isolated_before(text, start) {
        return None;
    }
    let first = character_after(text, start + prefix)?;
    if !('1'..='9').contains(&first) {
        return None;
    }
    let mut group_ends: Vec<(usize, usize)> = Vec::new();
    let mut position = start + prefix;
    let mut digits = 0;
    while let Some((end, count)) = group_at(text, position) {
        digits += count;
        if digits > MAX_DIGITS {
            break;
        }
        group_ends.push((end, digits));
        position = match character_after(text, end) {
            Some(' ' | '-') if starts_group(text, end + 1) => end + 1,
            Some(character) if character == '(' || character.is_ascii_digit() => end,
            _ => break,
        };
    }
    group_ends
        .iter()
        .rev()
        .find(|(end, count)| *count >= MIN_DIGITS && isolated_after(text, *end))
        .map(|(end, _)| *end)
}

fn starts_group(text: &str, position: usize) -> bool {
    character_after(text, position)
        .is_some_and(|character| character.is_ascii_digit() || character == '(')
}

/// One group at `position`: digits, or digits in parentheses. Returns its
/// end and its number of digits.
fn group_at(text: &str, position: usize) -> Option<(usize, usize)> {
    let opened = character_after(text, position) == Some('(');
    let digits_start = if opened { position + 1 } else { position };
    let digits_end = super::run_end(text, digits_start, MAX_DIGITS, |character| {
        character.is_ascii_digit()
    })
    .unwrap_or(digits_start + MAX_DIGITS + 1);
    let count = digits_end - digits_start;
    if count == 0 || count > MAX_DIGITS {
        return None;
    }
    if opened {
        if character_after(text, digits_end) != Some(')') {
            return None;
        }
        return Some((digits_end + 1, count));
    }
    Some((digits_end, count))
}

/// `(`?, `0`, ` `?, a digit from 2 to 5 or 8, two digits, `)`?,
/// then seven digits each after at most one space or `-`.
fn national_at(text: &str, start: usize) -> Option<usize> {
    if !isolated_before(text, start) {
        return None;
    }
    let mut position = start;
    let mut take = |accept: &dyn Fn(char) -> bool, optional: bool| -> Option<bool> {
        match character_after(text, position) {
            Some(character) if accept(character) => {
                position += character.len_utf8();
                Some(true)
            }
            _ if optional => Some(false),
            _ => None,
        }
    };
    take(&|character| character == '(', true)?;
    take(&|character| character == '0', false)?;
    take(&|character| character == ' ', true)?;
    take(&|character| matches!(character, '2'..='5' | '8'), false)?;
    take(&|character| character.is_ascii_digit(), false)?;
    take(&|character| character.is_ascii_digit(), false)?;
    take(&|character| character == ')', true)?;
    for _ in 0..NATIONAL_TRAILING_DIGITS {
        take(&|character| character == ' ' || character == '-', true)?;
        take(&|character| character.is_ascii_digit(), false)?;
    }
    isolated_after(text, position).then_some(position)
}
