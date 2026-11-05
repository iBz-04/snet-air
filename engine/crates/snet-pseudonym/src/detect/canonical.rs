use std::fmt::Write;
use std::net::IpAddr;

use zeroize::Zeroizing;

use super::{Builtin, Source};
use crate::text::fold;

const LONGEST_ADDRESS_TEXT: usize = 45;

/// The canonical form of one match's text; `None` only for text the
/// detector of `source` would not have accepted.
pub(crate) fn canonical(source: Source, matched: &str) -> Option<Zeroizing<String>> {
    let form = match source {
        Source::Dictionary(_) | Source::Pattern(_) | Source::Builtin(Builtin::Email) => {
            fold(matched)
        }
        Source::Builtin(Builtin::Phone) => phone(matched),
        Source::Builtin(Builtin::Iban) => kept(matched, |character| *character != ' '),
        Source::Builtin(Builtin::PaymentCard | Builtin::TrIdentity) => {
            kept(matched, char::is_ascii_digit)
        }
        Source::Builtin(Builtin::IpAddress) => address(matched)?,
        Source::Builtin(Builtin::MacAddress) => {
            let size = matched.chars().filter(char::is_ascii_hexdigit).count();
            let mut hex = String::with_capacity(size);
            hex.extend(
                matched
                    .chars()
                    .filter(char::is_ascii_hexdigit)
                    .map(|character| character.to_ascii_lowercase()),
            );
            Zeroizing::new(hex)
        }
    };
    Some(form)
}

fn kept(matched: &str, keep: impl Fn(&char) -> bool) -> Zeroizing<String> {
    let size = matched.chars().filter(&keep).count();
    let mut text = String::with_capacity(size);
    text.extend(matched.chars().filter(keep));
    Zeroizing::new(text)
}

fn address(matched: &str) -> Option<Zeroizing<String>> {
    let parsed: IpAddr = matched.parse().ok()?;
    let mut text = Zeroizing::new(String::with_capacity(LONGEST_ADDRESS_TEXT));
    write!(text, "{parsed}").ok()?;
    Some(text)
}

/// `+` and the digits; a national `0` becomes `+90`, `00` becomes `+`, and
/// a `(0)` group right after the first group of digits is left out.
fn phone(matched: &str) -> Zeroizing<String> {
    let international = matched.starts_with('+') || matched.starts_with("00");
    let size = 3 + matched.chars().filter(char::is_ascii_digit).count();
    let mut text = Zeroizing::new(String::with_capacity(size));
    if !international {
        text.push_str("+90");
        let mut digits = matched.chars().filter(char::is_ascii_digit);
        digits.next();
        text.extend(digits);
        return text;
    }
    text.push('+');
    let body = matched.strip_prefix('+').unwrap_or_else(|| &matched[2..]);
    let mut group_index = 0;
    let mut characters = body.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if character == '(' {
            if group_index == 1 && body[index..].starts_with("(0)") {
                characters.next();
                characters.next();
                group_index += 1;
            }
            continue;
        }
        if character.is_ascii_digit() {
            text.push(character);
            let ends_group = characters
                .peek()
                .is_none_or(|(_, next)| !next.is_ascii_digit());
            if ends_group {
                group_index += 1;
            }
        }
    }
    text
}
