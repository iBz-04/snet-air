pub(crate) mod dictionary;
pub(crate) mod file;
mod pattern;
mod task_terms;

use std::path::Path;

use aho_corasick::{AhoCorasick, MatchKind};
use sha2::{Digest, Sha256};

use crate::detect::{Builtin, Source};
use crate::errors::PolicyError;
use crate::outbound::Destination;
use file::invalid;
pub(crate) use pattern::PolicyPattern;

/// The policy used when the user names none: every built-in detector on,
/// no dictionaries, no patterns and nothing passed through in clear.
pub const BUILTIN_POLICY: &str = include_str!("builtin.toml");

const MAX_DICTIONARIES: usize = 16;
const MAX_TERMS: usize = 200_000;
const MAX_FOLDED_TERM_BYTES: usize = 8 * 1024 * 1024;
const MAX_PATTERNS: usize = 64;
const MAX_PASSTHROUGH_RULES: usize = 32;
const MAX_CATEGORY_CHARACTERS: usize = 32;
const MAX_ENDPOINT_NAME: usize = 64;
const MAX_HOST: usize = 253;
const MAX_LEGAL_BASIS: usize = 200;

/// A rule that lets one category reach one endpoint in clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassthroughRule {
    pub category: String,
    pub endpoint_name: String,
    pub host: String,
    pub legal_basis: String,
}

/// A loaded policy: which detectors run, its dictionaries and
/// patterns, the passthrough rules and the digest that names them all.
pub struct Policy {
    pub(crate) enabled: Vec<Builtin>,
    pub(crate) dictionary_categories: Vec<String>,
    pub(crate) automaton: Option<AhoCorasick>,
    /// The folded terms the automaton was built from, by automaton index.
    pub(crate) terms: Vec<zeroize::Zeroizing<String>>,
    pub(crate) term_dictionary: Vec<usize>,
    pub(crate) patterns: Vec<PolicyPattern>,
    pub(crate) passthrough: Vec<PassthroughRule>,
    pub(crate) digest: [u8; 32],
}

impl std::fmt::Debug for Policy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Policy")
            .field("enabled", &self.enabled)
            .field("dictionaries", &self.dictionary_categories.len())
            .field("patterns", &self.patterns.len())
            .field("passthrough", &self.passthrough.len())
            .finish()
    }
}

impl Policy {
    pub fn load(path: &Path) -> Result<Self, PolicyError> {
        let bytes = std::fs::read(path).map_err(|error| PolicyError::Read {
            path: path.to_path_buf(),
            cause: error.to_string(),
        })?;
        let text = std::str::from_utf8(&bytes).map_err(|_| PolicyError::Read {
            path: path.to_path_buf(),
            cause: "the file is not UTF-8".to_string(),
        })?;
        Self::parse(text)
    }

    pub fn builtin() -> Result<Self, PolicyError> {
        Self::parse(BUILTIN_POLICY)
    }

    pub fn parse(text: &str) -> Result<Self, PolicyError> {
        let parsed = file::parse(text)?;
        if parsed.dictionaries.len() > MAX_DICTIONARIES {
            return Err(PolicyError::Limit {
                limit: "16 dictionaries",
            });
        }
        if parsed.patterns.len() > MAX_PATTERNS {
            return Err(PolicyError::Limit {
                limit: "64 policy patterns",
            });
        }
        if parsed.passthrough.len() > MAX_PASSTHROUGH_RULES {
            return Err(PolicyError::Limit {
                limit: "32 passthrough rules",
            });
        }

        let mut hasher = Sha256::new();
        add_to_digest(&mut hasher, text.as_bytes());

        let mut dictionary_categories = Vec::with_capacity(parsed.dictionaries.len());
        let mut all_terms = Vec::new();
        let mut term_dictionary = Vec::new();
        let mut folded_bytes = 0usize;
        for (position, entry) in parsed.dictionaries.iter().enumerate() {
            check_policy_category(
                &entry.category,
                &format!("dictionaries[{position}].category"),
            )?;
            let read = dictionary::read(&entry.path, &format!("dictionaries[{position}].path"))?;
            add_to_digest(&mut hasher, &read.bytes);
            for term in read.terms {
                folded_bytes += term.len();
                all_terms.push(term);
                term_dictionary.push(position);
            }
            if all_terms.len() > MAX_TERMS {
                return Err(PolicyError::Limit {
                    limit: "200 000 terms",
                });
            }
            if folded_bytes > MAX_FOLDED_TERM_BYTES {
                return Err(PolicyError::Limit {
                    limit: "8 MiB of folded term bytes",
                });
            }
            dictionary_categories.push(entry.category.clone());
        }
        let automaton = term_automaton(&all_terms)?;

        let mut patterns = Vec::with_capacity(parsed.patterns.len());
        for (position, entry) in parsed.patterns.iter().enumerate() {
            check_policy_category(&entry.category, &format!("patterns[{position}].category"))?;
            patterns.push(pattern::compile(position, &entry.category, &entry.regex)?);
        }

        let mut passthrough = Vec::with_capacity(parsed.passthrough.len());
        for (position, entry) in parsed.passthrough.into_iter().enumerate() {
            let field = |name: &str| format!("passthrough[{position}].{name}");
            let known = Builtin::ALL
                .iter()
                .any(|kind| kind.name() == entry.category)
                || dictionary_categories.contains(&entry.category)
                || patterns
                    .iter()
                    .any(|pattern| pattern.category == entry.category);
            if !known {
                return Err(invalid(
                    field("category"),
                    "names no category of this policy",
                ));
            }
            let endpoint_name_is_valid = (1..=MAX_ENDPOINT_NAME)
                .contains(&entry.endpoint_name.len())
                && entry.endpoint_name.chars().all(|character| {
                    character.is_ascii_alphanumeric() || "_-.".contains(character)
                });
            if !endpoint_name_is_valid {
                return Err(invalid(
                    field("endpoint_name"),
                    "must be 1 to 64 letters, digits, `_`, `-` or `.`",
                ));
            }
            let host_is_valid = (1..=MAX_HOST).contains(&entry.host.len())
                && entry
                    .host
                    .chars()
                    .all(|character| character.is_ascii_graphic());
            if !host_is_valid {
                return Err(invalid(
                    field("host"),
                    "must be 1 to 253 visible ASCII characters",
                ));
            }
            let basis_characters = entry.legal_basis.chars().count();
            let basis_is_valid = (1..=MAX_LEGAL_BASIS).contains(&basis_characters)
                && !entry.legal_basis.chars().any(char::is_control)
                && !entry.legal_basis.trim().is_empty();
            if !basis_is_valid {
                return Err(invalid(field("legal_basis"), "must be 1 to 200 characters"));
            }
            passthrough.push(PassthroughRule {
                category: entry.category,
                endpoint_name: entry.endpoint_name,
                host: entry.host,
                legal_basis: entry.legal_basis,
            });
        }

        Ok(Self {
            enabled: parsed.enabled,
            dictionary_categories,
            automaton,
            terms: all_terms,
            term_dictionary,
            patterns,
            passthrough,
            digest: hasher.finalize().into(),
        })
    }

    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    pub fn passthrough_rules(&self) -> &[PassthroughRule] {
        &self.passthrough
    }

    /// The rules whose endpoint name and host equal the destination's
    ///, by position.
    pub(crate) fn rules_for(&self, destination: &Destination) -> Vec<usize> {
        self.passthrough
            .iter()
            .enumerate()
            .filter(|(_, rule)| {
                rule.endpoint_name == destination.endpoint_name() && rule.host == destination.host()
            })
            .map(|(position, _)| position)
            .collect()
    }

    pub(crate) fn category_of(&self, source: Source) -> &str {
        match source {
            Source::Dictionary(index) => &self.dictionary_categories[index],
            Source::Pattern(index) => &self.patterns[index].category,
            Source::Builtin(kind) => kind.name(),
        }
    }
}

/// The matcher over every folded dictionary term, or none without terms.
fn term_automaton(
    terms: &[zeroize::Zeroizing<String>],
) -> Result<Option<AhoCorasick>, PolicyError> {
    if terms.is_empty() {
        return Ok(None);
    }
    AhoCorasick::builder()
        .match_kind(MatchKind::Standard)
        .build(terms.iter().map(|term| term.as_bytes()))
        .map(Some)
        .map_err(|_| PolicyError::Limit {
            limit: "the dictionary automaton's size",
        })
}

/// Each file as its length, big-endian, then its bytes.
fn add_to_digest(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

pub(crate) fn is_category_name(name: &str) -> bool {
    let mut characters = name.chars();
    (1..=MAX_CATEGORY_CHARACTERS).contains(&name.len())
        && characters
            .next()
            .is_some_and(|first| first.is_ascii_uppercase())
        && characters.all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
}

fn check_policy_category(name: &str, field: &str) -> Result<(), PolicyError> {
    if !is_category_name(name) {
        return Err(invalid(
            field,
            "must be 1 to 32 of `A`-`Z`, `0`-`9` and `_`, starting with a letter",
        ));
    }
    if Builtin::ALL.iter().any(|kind| kind.name() == name) {
        return Err(invalid(field, "must not be a built-in category's name"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
