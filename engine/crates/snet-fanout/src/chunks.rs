use std::ops::Range;

/// The byte length of `text` as `serde_json` writes it as a JSON
/// string, without its quotes.
pub(crate) fn escaped_len(text: &str) -> usize {
    text.chars().map(escaped_char_len).sum()
}

fn escaped_char_len(character: char) -> usize {
    match character {
        '"' | '\\' | '\u{08}' | '\u{0C}' | '\n' | '\r' | '\t' => 2,
        control if control < '\u{20}' => 6,
        other => other.len_utf8(),
    }
}

/// The largest even escaped length from 2 to 1 048 576 for which
/// `fits` holds, found by bisection, or `None` when even 2 does not fit.
pub(crate) fn chunk_size(fits: impl Fn(usize) -> bool) -> Option<usize> {
    let (mut low, mut high) = (0usize, 524_288usize);
    while low < high {
        let middle = (low + high).div_ceil(2);
        if fits(middle * 2) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    (low > 0).then_some(low * 2)
}

/// A document split into byte ranges, each within `size` by escaped
/// length, ending after the last line break that fits, else after the last
/// whitespace that fits, else at the last character boundary that fits.
pub(crate) fn split(text: &str, size: usize) -> Vec<Range<usize>> {
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let rest = &text[start..];
        if escaped_len(rest) <= size {
            chunks.push(start..text.len());
            break;
        }
        let mut fitting_end = start;
        let mut used = 0;
        let mut last_line_break = None;
        let mut last_whitespace = None;
        for (offset, character) in rest.char_indices() {
            let cost = escaped_char_len(character);
            if used + cost > size {
                break;
            }
            used += cost;
            fitting_end = start + offset + character.len_utf8();
            if character == '\n' {
                last_line_break = Some(fitting_end);
            } else if character.is_whitespace() {
                last_whitespace = Some(fitting_end);
            }
        }
        let end = last_line_break.or(last_whitespace).unwrap_or(fitting_end);
        chunks.push(start..end);
        start = end;
    }
    chunks
}

#[cfg(test)]
mod tests;
