mod canonical;
mod email;
mod iban;
mod network;
mod numbers;
mod phone;

use crate::policy::Policy;
use crate::text::{character_after, character_before, fold_with_origins, is_word_character};

pub(crate) use canonical::canonical;

/// The built-in categories in their ranking order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Builtin {
    Email,
    Iban,
    PaymentCard,
    TrIdentity,
    Phone,
    IpAddress,
    MacAddress,
}

impl Builtin {
    pub const ALL: [Builtin; 7] = [
        Builtin::Email,
        Builtin::Iban,
        Builtin::PaymentCard,
        Builtin::TrIdentity,
        Builtin::Phone,
        Builtin::IpAddress,
        Builtin::MacAddress,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Builtin::Email => "EMAIL",
            Builtin::Iban => "IBAN",
            Builtin::PaymentCard => "PAYMENT_CARD",
            Builtin::TrIdentity => "TR_IDENTITY",
            Builtin::Phone => "PHONE",
            Builtin::IpAddress => "IP_ADDRESS",
            Builtin::MacAddress => "MAC_ADDRESS",
        }
    }

    /// The candidate ending at `start`'s longest valid end, if any.
    fn candidate_at(self, text: &str, start: usize) -> Option<usize> {
        match self {
            Builtin::Email => email::candidate_at(text, start),
            Builtin::Iban => iban::candidate_at(text, start),
            Builtin::PaymentCard => numbers::card_at(text, start),
            Builtin::TrIdentity => numbers::identity_at(text, start),
            Builtin::Phone => phone::candidate_at(text, start),
            Builtin::IpAddress => network::ip_at(text, start),
            Builtin::MacAddress => network::mac_at(text, start),
        }
    }
}

/// Where a match came from; the derived order is the ranking:
/// dictionaries, then policy patterns, then built-ins, each in their order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Source {
    Dictionary(usize),
    Pattern(usize),
    Builtin(Builtin),
}

/// A match as a byte range of the NFC text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Detection {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) source: Source,
}

/// Every match of every enabled detector, dictionary and policy pattern.
pub(crate) fn detect_all(policy: &Policy, text: &str) -> Vec<Detection> {
    let mut found = Vec::new();
    detect_dictionaries(policy, text, &mut found);
    detect_patterns(policy, text, &mut found);
    for kind in &policy.enabled {
        detect_builtin(*kind, text, &mut found);
    }
    found
}

/// Built-in detectors try every start position.
pub(crate) fn detect_builtin(kind: Builtin, text: &str, found: &mut Vec<Detection>) {
    for (start, _) in text.char_indices() {
        if let Some(end) = kind.candidate_at(text, start) {
            found.push(Detection {
                start,
                end,
                source: Source::Builtin(kind),
            });
        }
    }
}

/// The characters before and after are absent or not word
/// characters.
pub(crate) fn on_word_boundaries(text: &str, start: usize, end: usize) -> bool {
    !character_before(text, start).is_some_and(is_word_character)
        && !character_after(text, end).is_some_and(is_word_character)
}

fn detect_dictionaries(policy: &Policy, text: &str, found: &mut Vec<Detection>) {
    let Some(automaton) = &policy.automaton else {
        return;
    };
    let folded = fold_with_origins(text);
    for matched in automaton.find_overlapping_iter(folded.text.as_bytes()) {
        let start = folded.origin_start[matched.start()];
        let end = folded.origin_end[matched.end() - 1];
        if on_word_boundaries(text, start, end) {
            found.push(Detection {
                start,
                end,
                source: Source::Dictionary(policy.term_dictionary[matched.pattern().as_usize()]),
            });
        }
    }
}

/// Every start position whose preceding character is a boundary;
/// the wrapped pattern carries the end boundary, and an unanchored search
/// from a position finds the leftmost start at or after it, so the
/// positions in between can hold no match.
fn detect_patterns(policy: &Policy, text: &str, found: &mut Vec<Detection>) {
    for (index, site) in policy.patterns.iter().enumerate() {
        let mut position = 0;
        while position <= text.len() {
            let Some(captures) = site.regex.captures_at(text, position) else {
                break;
            };
            let Some(value) = captures.get(1) else {
                break;
            };
            let start = value.start();
            if !character_before(text, start).is_some_and(is_word_character) {
                found.push(Detection {
                    start,
                    end: value.end(),
                    source: Source::Pattern(index),
                });
            }
            match character_after(text, start) {
                Some(character) => position = start + character.len_utf8(),
                None => break,
            }
        }
    }
}

/// Numeric isolation before a match.
pub(crate) fn isolated_before(text: &str, start: usize) -> bool {
    let mut before = text[..start].chars().rev();
    match before.next() {
        None => true,
        Some(character) if character.is_ascii_digit() => false,
        Some('.' | ',') => !before
            .next()
            .is_some_and(|character| character.is_ascii_digit()),
        Some(_) => true,
    }
}

/// Numeric isolation after a match.
pub(crate) fn isolated_after(text: &str, end: usize) -> bool {
    let mut after = text[end..].chars();
    match after.next() {
        None => true,
        Some(character) if character.is_ascii_digit() => false,
        Some('.' | ',') => !after
            .next()
            .is_some_and(|character| character.is_ascii_digit()),
        Some(_) => true,
    }
}

/// Not touching a word character on either side.
pub(crate) fn untouched(text: &str, start: usize, end: usize) -> bool {
    on_word_boundaries(text, start, end)
}

/// The end of the run of characters matching `accept` from `start`, or
/// `None` when the run is longer than `limit` characters.
pub(crate) fn run_end(
    text: &str,
    start: usize,
    limit: usize,
    accept: impl Fn(char) -> bool,
) -> Option<usize> {
    let mut end = start;
    for (count, character) in text[start..].chars().enumerate() {
        if !accept(character) {
            return Some(end);
        }
        if count == limit {
            return None;
        }
        end += character.len_utf8();
    }
    Some(end)
}

#[cfg(test)]
mod tests;
