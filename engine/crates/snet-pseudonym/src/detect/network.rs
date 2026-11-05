use std::net::Ipv6Addr;

use super::{isolated_after, isolated_before, untouched};
use crate::text::character_after;

const MAX_IPV6_CHARACTERS: usize = 45;

pub(super) fn ip_at(text: &str, start: usize) -> Option<usize> {
    ipv4_at(text, start).or_else(|| ipv6_at(text, start))
}

/// Four numbers from 0 to 255 without leading zeros.
fn ipv4_at(text: &str, start: usize) -> Option<usize> {
    if !isolated_before(text, start) {
        return None;
    }
    let mut position = start;
    for octet in 0..4 {
        if octet > 0 {
            if character_after(text, position) != Some('.') {
                return None;
            }
            position += 1;
        }
        let end = super::run_end(text, position, 3, |character| character.is_ascii_digit())?;
        let number = &text[position..end];
        let valid = !number.is_empty()
            && !(number.len() > 1 && number.starts_with('0'))
            && number.parse::<u16>().is_ok_and(|value| value <= 255);
        if !valid {
            return None;
        }
        position = end;
    }
    isolated_after(text, position).then_some(position)
}

/// A run of hex digits, `:` and `.` with trailing `.` and one
/// trailing lone `:` removed. A run that starts with one lone `:` is the
/// same address as the run starting one character later, which the search
/// at that position finds, so a start on a lone `:` yields nothing.
fn ipv6_at(text: &str, start: usize) -> Option<usize> {
    let first = character_after(text, start)?;
    let lone_colon = first == ':' && !text[start..].starts_with("::");
    if lone_colon || !(first.is_ascii_hexdigit() || first == ':') {
        return None;
    }
    let run_end = super::run_end(text, start, MAX_IPV6_CHARACTERS + 2, |character| {
        character.is_ascii_hexdigit() || character == ':' || character == '.'
    })?;
    let mut end = run_end;
    while end > start && text[..end].ends_with('.') {
        end -= 1;
    }
    let trimmed = &text[start..end];
    if trimmed.ends_with(':') && !trimmed.ends_with("::") {
        end -= 1;
    }
    let address = &text[start..end];
    let valid = address.len() <= MAX_IPV6_CHARACTERS
        && address.matches(':').count() >= 2
        && address.chars().any(|character| character.is_ascii_digit())
        && untouched(text, start, end)
        && address.parse::<Ipv6Addr>().is_ok();
    valid.then_some(end)
}

/// Six or eight pairs of hex digits joined by one `:` or `-`, or
/// three groups of four joined by `.`, not touching a word character.
pub(super) fn mac_at(text: &str, start: usize) -> Option<usize> {
    if !character_after(text, start).is_some_and(|character| character.is_ascii_hexdigit()) {
        return None;
    }
    let hex_run = |position: usize, length: usize| -> Option<usize> {
        let end = super::run_end(text, position, length, |character| {
            character.is_ascii_hexdigit()
        })?;
        (end - position == length).then_some(end)
    };
    let mut pair_ends = Vec::new();
    if let Some(first) = hex_run(start, 2) {
        pair_ends.push(first);
        let separator =
            character_after(text, first).filter(|character| *character == ':' || *character == '-');
        if let Some(separator) = separator {
            let mut position = first;
            while pair_ends.len() < 8 && character_after(text, position) == Some(separator) {
                let Some(end) = hex_run(position + 1, 2) else {
                    break;
                };
                pair_ends.push(end);
                position = end;
            }
        }
    }
    for count in [8, 6] {
        if let Some(end) = pair_ends.get(count - 1)
            && untouched(text, start, *end)
        {
            return Some(*end);
        }
    }
    let mut position = start;
    for group in 0..3 {
        if group > 0 {
            if character_after(text, position) != Some('.') {
                return None;
            }
            position += 1;
        }
        position = hex_run(position, 4)?;
    }
    untouched(text, start, position).then_some(position)
}
