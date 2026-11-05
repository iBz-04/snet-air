use std::collections::HashSet;

use zeroize::Zeroizing;

use super::{MAX_ENTRIES, MAX_VALUE_BYTES, TaskMapping};
use crate::errors::PseudonymError;
use crate::policy::is_category_name;

const VERSION: u8 = 1;
const WORD: usize = 4;
/// A category's smallest encoding: a name length, a one-byte name, a
/// counter and an entry count.
const SMALLEST_CATEGORY: usize = WORD + 1 + WORD + WORD;
/// An entry's smallest encoding: category, number and two lengths.
const SMALLEST_ENTRY: usize = 4 * WORD;

fn refuse(reason: &'static str) -> PseudonymError {
    PseudonymError::MappingBytes { reason }
}

impl TaskMapping {
    /// The mapping layout, written into a buffer allocated at its exact size.
    pub fn to_bytes(&self) -> Zeroizing<Vec<u8>> {
        let size = 1
            + WORD
            + self
                .categories
                .iter()
                .map(|category| 3 * WORD + category.name.len())
                .sum::<usize>()
            + self
                .entries
                .iter()
                .map(|entry| 4 * WORD + entry.canonical.len() + entry.surface.len())
                .sum::<usize>();
        let mut bytes = Zeroizing::new(Vec::with_capacity(size));
        let word = |bytes: &mut Vec<u8>, value: usize| {
            bytes.extend_from_slice(&(value as u32).to_be_bytes());
        };
        bytes.push(VERSION);
        word(&mut bytes, self.categories.len());
        for (index, category) in self.categories.iter().enumerate() {
            word(&mut bytes, category.name.len());
            bytes.extend_from_slice(category.name.as_bytes());
            word(&mut bytes, category.counter as usize);
            let count = self
                .entries
                .iter()
                .filter(|entry| entry.category as usize == index)
                .count();
            word(&mut bytes, count);
        }
        for entry in &self.entries {
            word(&mut bytes, entry.category as usize);
            word(&mut bytes, entry.number as usize);
            word(&mut bytes, entry.canonical.len());
            bytes.extend_from_slice(entry.canonical.as_bytes());
            word(&mut bytes, entry.surface.len());
            bytes.extend_from_slice(entry.surface.as_bytes());
        }
        bytes
    }

    /// Reads the mapping layout with every check, refusing a bad
    /// length or count before allocating for it.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PseudonymError> {
        let mut reader = Reader { bytes, position: 0 };
        if reader.byte()? != VERSION {
            return Err(refuse("it has an unknown version"));
        }
        let category_count = reader.word()?;
        if category_count > reader.left() / SMALLEST_CATEGORY {
            return Err(refuse("it declares more categories than its bytes hold"));
        }
        let mut names = HashSet::new();
        let mut declared = Vec::with_capacity(category_count);
        let mut mapping = TaskMapping::new();
        for _ in 0..category_count {
            let name = reader.text()?;
            if !is_category_name(&name) || !names.insert(name.to_string()) {
                return Err(refuse("a category name is invalid or repeated"));
            }
            let counter = reader.word()?;
            let entries = reader.word()?;
            if counter != entries {
                return Err(refuse("a category's counter differs from its entries"));
            }
            mapping.categories.push(super::CategoryState {
                name: name.to_string(),
                counter: 0,
            });
            declared.push((counter, entries));
        }
        let total: usize = declared.iter().map(|(_, entries)| entries).sum();
        if total > MAX_ENTRIES {
            return Err(refuse("it holds more than 10 000 entries"));
        }
        if total > reader.left() / SMALLEST_ENTRY {
            return Err(refuse("it declares more entries than its bytes hold"));
        }
        let mut seen = vec![0usize; category_count];
        let mut value_bytes = 0usize;
        for _ in 0..total {
            let category = reader.word()?;
            let number = reader.word()?;
            let canonical = reader.text()?;
            let surface = reader.text()?;
            let Some((counter, _)) = declared.get(category) else {
                return Err(refuse("an entry names a category out of range"));
            };
            if number == 0 || number > *counter {
                return Err(refuse("a token number is 0 or above its counter"));
            }
            value_bytes += canonical.len() + surface.len();
            if value_bytes > MAX_VALUE_BYTES {
                return Err(refuse("it holds more than 4 MiB of values"));
            }
            let name = mapping.categories[category].name.clone();
            if mapping.find_token(&name, number as u32).is_some() {
                return Err(refuse("a token is repeated"));
            }
            if mapping.find_value(&name, &canonical).is_some() {
                return Err(refuse("a value is repeated"));
            }
            seen[category] += 1;
            mapping.insert(&name, number as u32, canonical, surface);
        }
        if seen
            .iter()
            .zip(&declared)
            .any(|(seen, (_, entries))| seen != entries)
        {
            return Err(refuse(
                "a category's entry count does not match its entries",
            ));
        }
        if reader.left() != 0 {
            return Err(refuse("it has trailing bytes"));
        }
        Ok(mapping)
    }
}

struct Reader<'b> {
    bytes: &'b [u8],
    position: usize,
}

impl Reader<'_> {
    fn left(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, length: usize) -> Result<&[u8], PseudonymError> {
        if length > self.left() {
            return Err(refuse("a length runs past its end"));
        }
        let taken = &self.bytes[self.position..self.position + length];
        self.position += length;
        Ok(taken)
    }

    fn byte(&mut self) -> Result<u8, PseudonymError> {
        Ok(self.take(1)?[0])
    }

    fn word(&mut self) -> Result<usize, PseudonymError> {
        let taken = self.take(WORD)?;
        Ok(u32::from_be_bytes([taken[0], taken[1], taken[2], taken[3]]) as usize)
    }

    fn text(&mut self) -> Result<Zeroizing<String>, PseudonymError> {
        let length = self.word()?;
        let taken = self.take(length)?;
        let text = std::str::from_utf8(taken).map_err(|_| refuse("a text is not UTF-8"))?;
        Ok(crate::text::exact(&[text]))
    }
}
