use snet_cloud::Provider;

use super::palette::{ANTHROPIC_CLAY, APPLE_GREEN, APPLE_GREY, Hue, MELIOUS_BLUE};

/// The name of who answered, shown above an answer in that answerer's
/// colour. A colour of `None` keeps the terminal's own text colour, which
/// renders a black-and-white identity on light and dark themes alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wordmark {
    label: &'static str,
    hue: Option<Hue>,
}

#[cfg(target_os = "macos")]
const LOCAL_LABEL: &str = "This Mac";
#[cfg(not(target_os = "macos"))]
const LOCAL_LABEL: &str = "This computer";

impl Wordmark {
    pub fn cloud(provider: Provider) -> Self {
        match provider {
            Provider::Anthropic => Self {
                label: "Anthropic",
                hue: Some(ANTHROPIC_CLAY),
            },
            Provider::Melious => Self {
                label: "Melious",
                hue: Some(MELIOUS_BLUE),
            },
            Provider::Openai => Self {
                label: "OpenAI",
                hue: None,
            },
            Provider::OpenaiCompatible => Self {
                label: "Custom server",
                hue: Some(APPLE_GREY),
            },
        }
    }

    pub fn local() -> Self {
        Self {
            label: LOCAL_LABEL,
            hue: Some(APPLE_GREEN),
        }
    }

    /// The label in sans-serif bold letters, bold and in its colour.
    pub fn styled(self) -> String {
        let style = match self.hue {
            Some(hue) => hue.style(),
            None => console::Style::new(),
        };
        format!("{}", style.bold().apply_to(sans_serif_bold(self.label)))
    }
}

/// `text` with every ASCII letter and digit swapped for its Unicode
/// mathematical sans-serif bold form, which most monospace fonts draw as a
/// separate bold sans face. Other characters stay as they are.
pub fn sans_serif_bold(text: &str) -> String {
    text.chars()
        .map(|character| sans_serif_bold_form(character).unwrap_or(character))
        .collect()
}

const SANS_BOLD_CAPITAL_A: u32 = 0x1D5D4;
const SANS_BOLD_SMALL_A: u32 = 0x1D5EE;
const SANS_BOLD_DIGIT_ZERO: u32 = 0x1D7EC;

/// The sans-serif bold form of an ASCII letter or digit; `None` for any
/// other character, which has no such form.
fn sans_serif_bold_form(character: char) -> Option<char> {
    let first = match character {
        'A'..='Z' => SANS_BOLD_CAPITAL_A + (u32::from(character) - u32::from('A')),
        'a'..='z' => SANS_BOLD_SMALL_A + (u32::from(character) - u32::from('a')),
        '0'..='9' => SANS_BOLD_DIGIT_ZERO + (u32::from(character) - u32::from('0')),
        _ => return None,
    };
    char::from_u32(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_digits_take_their_sans_serif_bold_form() {
        assert_eq!(sans_serif_bold("Anthropic"), "𝗔𝗻𝘁𝗵𝗿𝗼𝗽𝗶𝗰");
        assert_eq!(sans_serif_bold("OpenAI"), "𝗢𝗽𝗲𝗻𝗔𝗜");
        assert_eq!(sans_serif_bold("Zz09"), "𝗭𝘇𝟬𝟵");
        assert_eq!(sans_serif_bold("Custom server"), "𝗖𝘂𝘀𝘁𝗼𝗺 𝘀𝗲𝗿𝘃𝗲𝗿");
        assert_eq!(sans_serif_bold("snet-air"), "𝘀𝗻𝗲𝘁-𝗮𝗶𝗿");
    }

    #[test]
    fn every_answerer_has_its_name_and_colour() {
        let expected = [
            (Provider::Anthropic, "Anthropic", Some(ANTHROPIC_CLAY)),
            (Provider::Melious, "Melious", Some(MELIOUS_BLUE)),
            (Provider::Openai, "OpenAI", None),
            (
                Provider::OpenaiCompatible,
                "Custom server",
                Some(APPLE_GREY),
            ),
        ];
        for (provider, label, hue) in expected {
            assert_eq!(Wordmark::cloud(provider), Wordmark { label, hue });
        }
        assert_eq!(Wordmark::local().label, LOCAL_LABEL);
        assert_eq!(Wordmark::local().hue, Some(APPLE_GREEN));
    }

    #[test]
    fn the_wordmark_is_one_column_per_letter() {
        let wordmark = Wordmark::cloud(Provider::Melious).styled();
        assert_eq!(console::measure_text_width(&wordmark), "Melious".len());
    }
}
