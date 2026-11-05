use super::{isolated_after, isolated_before};
use crate::text::character_after;

const MAX_CARD_DIGITS: usize = 19;
const MIN_CARD_DIGITS: usize = 13;
const IDENTITY_DIGITS: usize = 11;

/// Card and identity numbers at one start: digits joined or split by one separator used the
/// same way throughout, trying shorter group counts.
pub(super) fn card_at(text: &str, start: usize) -> Option<usize> {
    if !character_after(text, start).is_some_and(|character| character.is_ascii_digit())
        || !isolated_before(text, start)
    {
        return None;
    }
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut separator: Option<char> = None;
    let mut position = start;
    let mut digits = 0;
    while let Some(end) = super::run_end(text, position, MAX_CARD_DIGITS, |character| {
        character.is_ascii_digit()
    }) {
        digits += end - position;
        if digits > MAX_CARD_DIGITS {
            break;
        }
        groups.push((end, digits));
        let next = character_after(text, end);
        let continues = matches!(next, Some(' ' | '-'))
            && separator.is_none_or(|chosen| Some(chosen) == next)
            && character_after(text, end + 1).is_some_and(|character| character.is_ascii_digit());
        if !continues {
            break;
        }
        separator = next;
        position = end + 1;
    }
    groups.iter().rev().find_map(|(end, count)| {
        let candidate = &text[start..*end];
        let valid = (MIN_CARD_DIGITS..=MAX_CARD_DIGITS).contains(count)
            && isolated_after(text, *end)
            && card_number_is_valid(candidate);
        valid.then_some(*end)
    })
}

fn digits_of(text: &str) -> impl DoubleEndedIterator<Item = u32> + '_ {
    text.chars().filter_map(|character| character.to_digit(10))
}

fn card_number_is_valid(candidate: &str) -> bool {
    let count = digits_of(candidate).count();
    let mut leading = [0u32; 4];
    for (slot, digit) in leading.iter_mut().zip(digits_of(candidate)) {
        *slot = digit;
    }
    let first = leading[0];
    let two = leading[0] * 10 + leading[1];
    let four = two * 100 + leading[2] * 10 + leading[3];
    let prefix_fits = if count == MIN_CARD_DIGITS {
        first == 4
    } else {
        (2..=6).contains(&first)
            || two == 81
            || two == 82
            || matches!(four, 8600 | 9704 | 9792 | 9860)
    };
    prefix_fits && luhn(candidate)
}

fn luhn(candidate: &str) -> bool {
    let sum: u32 = digits_of(candidate)
        .rev()
        .enumerate()
        .map(|(index, digit)| {
            if index % 2 == 1 {
                let doubled = digit * 2;
                if doubled > 9 { doubled - 9 } else { doubled }
            } else {
                digit
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// Eleven joined digits passing both check digits.
pub(super) fn identity_at(text: &str, start: usize) -> Option<usize> {
    if !character_after(text, start).is_some_and(|character| character.is_ascii_digit())
        || !isolated_before(text, start)
    {
        return None;
    }
    let end = super::run_end(text, start, IDENTITY_DIGITS, |character| {
        character.is_ascii_digit()
    })?;
    if end - start != IDENTITY_DIGITS || !isolated_after(text, end) {
        return None;
    }
    let mut digits = [0i32; IDENTITY_DIGITS];
    for (slot, byte) in digits.iter_mut().zip(text[start..end].bytes()) {
        *slot = i32::from(byte - b'0');
    }
    let odd = digits[0] + digits[2] + digits[4] + digits[6] + digits[8];
    let even = digits[1] + digits[3] + digits[5] + digits[7];
    let tenth = (odd * 7 - even).rem_euclid(10);
    let eleventh = digits[..10].iter().sum::<i32>() % 10;
    (digits[0] != 0 && digits[9] == tenth && digits[10] == eleventh).then_some(end)
}
