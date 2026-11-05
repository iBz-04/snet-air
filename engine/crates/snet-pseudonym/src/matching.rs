use zeroize::Zeroizing;

use crate::text::{fold, nfc};

/// NFC, then the matching fold: the form in which fan-out matches search terms
/// against chunks.
pub fn fold_for_matching(text: &str) -> Zeroizing<String> {
    fold(&nfc(text))
}

/// Whether the characters around `start..end` of `text` are absent
/// or are neither alphanumeric nor `_`.
pub fn on_word_boundaries(text: &str, start: usize, end: usize) -> bool {
    crate::detect::on_word_boundaries(text, start, end)
}

#[cfg(test)]
mod tests {
    use super::{fold_for_matching, on_word_boundaries};

    #[test]
    fn the_fold_normalises_and_folds_turkish_i() {
        assert_eq!(fold_for_matching("IŞIK İzmir").as_str(), "işik izmir");
        assert_eq!(fold_for_matching("Jose\u{301}").as_str(), "josé");
    }

    #[test]
    fn word_boundaries_refuse_letters_digits_and_underscores() {
        let text = "a pump, pumpkin and pump_3";
        assert!(on_word_boundaries(text, 2, 6));
        assert!(!on_word_boundaries(text, 8, 12));
        assert!(!on_word_boundaries(text, 22, 26));
    }
}
