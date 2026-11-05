use std::ops::Range;

const MIN_QUOTE_CHARACTERS: usize = 8;
const MAX_QUOTE_CHARACTERS: usize = 300;

/// A cited quote: its number, its document and its byte range there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cited {
    pub number: u32,
    pub document: usize,
    pub text: String,
    pub range: Range<usize>,
}

/// A cited passage of a local document, shown beside the answer.
#[derive(Clone, PartialEq, Eq)]
pub struct CitedPassage {
    pub citation: String,
    pub document: String,
    pub range: Range<usize>,
    pub text: String,
}

impl std::fmt::Debug for CitedPassage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CitedPassage")
            .field("citation", &self.citation)
            .field("range", &self.range)
            .finish()
    }
}

/// The byte range of a quote's first occurrence in `text`, when the quote
/// is 8 to 300 scalar values.
pub fn quote_range(text: &str, quote: &str) -> Option<Range<usize>> {
    let characters = quote.chars().count();
    if !(MIN_QUOTE_CHARACTERS..=MAX_QUOTE_CHARACTERS).contains(&characters) {
        return None;
    }
    let start = text.find(quote)?;
    Some(start..start + quote.len())
}

/// The citations of one task, keyed by document and quote text.
#[derive(Default)]
pub struct Citations {
    cited: Vec<Cited>,
}

impl Citations {
    /// The number of a quote, giving an earlier quote of the same document
    /// and text its old number and range.
    pub fn cite(&mut self, document: usize, text: &str, range: Range<usize>) -> u32 {
        if let Some(known) = self
            .cited
            .iter()
            .find(|cited| cited.document == document && cited.text == text)
        {
            return known.number;
        }
        let number = self.cited.len() as u32 + 1;
        self.cited.push(Cited {
            number,
            document,
            text: text.to_string(),
            range,
        });
        number
    }

    /// The citation of a document's quote, if it has one.
    pub fn find_by(&self, document: usize, text: &str) -> Option<&Cited> {
        self.cited
            .iter()
            .find(|cited| cited.document == document && cited.text == text)
    }

    pub fn len(&self) -> usize {
        self.cited.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cited.is_empty()
    }

    pub fn numbers(&self) -> Vec<u32> {
        self.cited.iter().map(|cited| cited.number).collect()
    }

    /// The citation named `C<n>`.
    pub fn find(&self, name: &str) -> Option<&Cited> {
        let number: u32 = name.strip_prefix('C')?.parse().ok()?;
        self.cited.iter().find(|cited| cited.number == number)
    }

    /// Whether `text` holds the text of any cited quote.
    pub fn copied_into(&self, text: &str) -> bool {
        self.cited
            .iter()
            .any(|cited| text.contains(cited.text.as_str()))
    }
}
