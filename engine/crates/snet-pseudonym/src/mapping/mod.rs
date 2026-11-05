mod bytes;

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

use zeroize::Zeroizing;

use crate::errors::PseudonymError;
use crate::text::letters_and_digits;

pub(crate) const MAX_ENTRIES: usize = 10_000;
pub(crate) const MAX_VALUE_BYTES: usize = 4 * 1024 * 1024;
const SPELLED_MINIMUM: usize = 6;
const PHONE_TAIL_DIGITS: usize = 10;

pub(crate) struct CategoryState {
    pub(crate) name: String,
    pub(crate) counter: u32,
}

/// One pseudonymised value. Each text sits in its own wiped buffer, so the
/// vector holding entries moves only their handles when it grows.
pub(crate) struct Entry {
    pub(crate) category: u32,
    pub(crate) number: u32,
    pub(crate) canonical: Zeroizing<String>,
    pub(crate) surface: Zeroizing<String>,
}

/// The values of one task and their tokens. It is never serialised except
/// into a wiped buffer of exact size, and its `Debug` shows only
/// counts.
pub struct TaskMapping {
    pub(crate) categories: Vec<CategoryState>,
    pub(crate) entries: Vec<Entry>,
    by_value: HashMap<u64, Vec<usize>>,
    by_token: HashMap<(u32, u32), usize>,
    hasher: RandomState,
    value_bytes: usize,
}

impl std::fmt::Debug for TaskMapping {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TaskMapping")
            .field("categories", &self.categories.len())
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl Default for TaskMapping {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskMapping {
    pub fn new() -> Self {
        Self {
            categories: Vec::new(),
            entries: Vec::new(),
            by_value: HashMap::new(),
            by_token: HashMap::new(),
            hasher: RandomState::new(),
            value_bytes: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn value_key(&self, category: &str, canonical: &str) -> u64 {
        self.hasher.hash_one((category, canonical))
    }

    fn category_index(&self, name: &str) -> Option<u32> {
        self.categories
            .iter()
            .position(|category| category.name == name)
            .and_then(|index| u32::try_from(index).ok())
    }

    fn find_value(&self, category: &str, canonical: &str) -> Option<&Entry> {
        let index = self.category_index(category)?;
        self.by_value
            .get(&self.value_key(category, canonical))?
            .iter()
            .map(|position| &self.entries[*position])
            .find(|entry| entry.category == index && entry.canonical.as_str() == canonical)
    }

    pub(crate) fn find_token(&self, category: &str, number: u32) -> Option<&Entry> {
        let index = self.category_index(category)?;
        self.by_token
            .get(&(index, number))
            .map(|position| &self.entries[*position])
    }

    pub(crate) fn category_name(&self, entry: &Entry) -> &str {
        &self.categories[entry.category as usize].name
    }

    fn insert(
        &mut self,
        category: &str,
        number: u32,
        canonical: Zeroizing<String>,
        surface: Zeroizing<String>,
    ) {
        let index = match self.category_index(category) {
            Some(index) => index,
            None => {
                self.categories.push(CategoryState {
                    name: category.to_string(),
                    counter: 0,
                });
                (self.categories.len() - 1) as u32
            }
        };
        let state = &mut self.categories[index as usize];
        state.counter = state.counter.max(number);
        let key = self.value_key(category, &canonical);
        let position = self.entries.len();
        self.value_bytes += canonical.len() + surface.len();
        self.entries.push(Entry {
            category: index,
            number,
            canonical,
            surface,
        });
        self.by_value.entry(key).or_default().push(position);
        self.by_token.insert((index, number), position);
    }

    /// Whether `text`, reduced to folded letters and digits, holds
    /// a value's surface or canonical form of six or more such characters,
    /// or the last ten digits of a phone number's canonical form.
    pub fn holds_spelled_value(&self, text: &str) -> bool {
        let reduced = letters_and_digits(text);
        let holds = |form: &str| {
            let reduced_form = letters_and_digits(form);
            reduced_form.chars().count() >= SPELLED_MINIMUM
                && reduced.contains(reduced_form.as_str())
        };
        self.entries.iter().any(|entry| {
            let phone_tail = self.category_name(entry) == "PHONE" && {
                let digits: Zeroizing<String> = letters_and_digits(&entry.canonical);
                let count = digits.chars().count();
                count >= PHONE_TAIL_DIGITS && reduced.contains(&digits[count - PHONE_TAIL_DIGITS..])
            };
            holds(&entry.surface) || holds(&entry.canonical) || phone_tail
        })
    }
}

/// Values found during one call, added to the mapping only when the whole
/// call succeeds.
pub(crate) struct Staging {
    added: Vec<(String, u32, Zeroizing<String>, Zeroizing<String>)>,
    added_by_value: HashMap<u64, Vec<usize>>,
    counters: Vec<(String, u32)>,
    added_bytes: usize,
}

impl Staging {
    pub(crate) fn new() -> Self {
        Self {
            added: Vec::new(),
            added_by_value: HashMap::new(),
            counters: Vec::new(),
            added_bytes: 0,
        }
    }

    /// The token number for a value: the one the mapping or this call
    /// already gave it, or the category's next.
    pub(crate) fn number_for(
        &mut self,
        mapping: &TaskMapping,
        category: &str,
        canonical: Zeroizing<String>,
        surface: &str,
    ) -> Result<u32, PseudonymError> {
        if let Some(entry) = mapping.find_value(category, &canonical) {
            return Ok(entry.number);
        }
        let key = mapping.value_key(category, &canonical);
        let staged = self.added_by_value.get(&key).and_then(|positions| {
            positions
                .iter()
                .map(|position| &self.added[*position])
                .find(|(name, _, form, _)| name == category && form.as_str() == canonical.as_str())
        });
        if let Some((_, number, _, _)) = staged {
            return Ok(*number);
        }
        if mapping.entries.len() + self.added.len() >= MAX_ENTRIES {
            return Err(PseudonymError::MappingFull {
                limit: "10 000 entries",
            });
        }
        let bytes = canonical.len() + surface.len();
        if mapping.value_bytes + self.added_bytes + bytes > MAX_VALUE_BYTES {
            return Err(PseudonymError::MappingFull {
                limit: "4 MiB of values",
            });
        }
        let base = mapping
            .category_index(category)
            .map_or(0, |index| mapping.categories[index as usize].counter);
        let number = match self.counters.iter_mut().find(|(name, _)| name == category) {
            Some((_, counter)) => {
                *counter += 1;
                *counter
            }
            None => {
                self.counters.push((category.to_string(), base + 1));
                base + 1
            }
        };
        self.added_bytes += bytes;
        self.added_by_value
            .entry(key)
            .or_default()
            .push(self.added.len());
        self.added.push((
            category.to_string(),
            number,
            canonical,
            crate::text::exact(&[surface]),
        ));
        Ok(number)
    }

    pub(crate) fn commit(self, mapping: &mut TaskMapping) {
        for (category, number, canonical, surface) in self.added {
            mapping.insert(&category, number, canonical, surface);
        }
    }
}

#[cfg(test)]
mod tests;
