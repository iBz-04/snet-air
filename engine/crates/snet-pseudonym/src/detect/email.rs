use crate::text::character_after;

const MAX_LOCAL_CHARACTERS: usize = 64;
const MAX_LABEL_CHARACTERS: usize = 63;
const MAX_LABELS: usize = 127;
const MAX_ADDRESS_CHARACTERS: usize = 254;

fn is_local_character(character: char) -> bool {
    character.is_alphabetic()
        || character.is_ascii_digit()
        || "!#$%&'*+/=?^_`{|}~.-".contains(character)
}

fn is_label_character(character: char) -> bool {
    character.is_alphabetic() || character.is_ascii_digit() || character == '-'
}

/// An email address at one start: the local part up to `@`, then the longest run of
/// labels that forms a valid domain and is not followed by a letter,
/// digit, `-` or `_`.
pub(super) fn candidate_at(text: &str, start: usize) -> Option<usize> {
    let first = character_after(text, start)?;
    if first == '.' || !is_local_character(first) {
        return None;
    }
    let at = super::run_end(text, start, MAX_LOCAL_CHARACTERS, is_local_character)?;
    if character_after(text, at) != Some('@') {
        return None;
    }
    let local = &text[start..at];
    if local.ends_with('.') || local.contains("..") {
        return None;
    }

    let mut label_ends = Vec::new();
    let mut position = at + 1;
    loop {
        let end = super::run_end(text, position, MAX_LABEL_CHARACTERS, is_label_character)?;
        if end == position {
            break;
        }
        label_ends.push((position, end));
        if label_ends.len() == MAX_LABELS {
            break;
        }
        let continues = character_after(text, end) == Some('.')
            && character_after(text, end + 1).is_some_and(is_label_character);
        if !continues {
            break;
        }
        position = end + 1;
    }

    for count in (2..=label_ends.len()).rev() {
        let labels = &label_ends[..count];
        let end = labels[count - 1].1;
        let every_label_valid = labels.iter().all(|(label_start, label_end)| {
            let label = &text[*label_start..*label_end];
            !label.starts_with('-') && !label.ends_with('-')
        });
        let (last_start, last_end) = labels[count - 1];
        let last = &text[last_start..last_end];
        let last_characters = last.chars().count();
        let last_is_valid = (2..=MAX_LABEL_CHARACTERS).contains(&last_characters)
            && last.chars().all(char::is_alphabetic);
        let short_enough = text[start..end].chars().count() <= MAX_ADDRESS_CHARACTERS;
        let free_after = !character_after(text, end).is_some_and(|character| {
            character.is_alphabetic()
                || character.is_ascii_digit()
                || character == '-'
                || character == '_'
        });
        if every_label_valid && last_is_valid && short_enough && free_after {
            return Some(end);
        }
    }
    None
}
