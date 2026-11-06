use regex::{Regex, RegexBuilder};

use crate::errors::PolicyError;

pub(crate) const MAX_PATTERN_BYTES: usize = 1024;
const MAX_MATCH_BYTES: usize = 256;
const COMPILED_SIZE_LIMIT: usize = 1024 * 1024;
const LAZY_DFA_LIMIT: usize = 2 * 1024 * 1024;

/// A policy pattern wrapped so its end boundary takes part in choosing the
/// match; group 1 is the value.
#[derive(Clone)]
pub(crate) struct PolicyPattern {
    pub(crate) category: String,
    pub(crate) regex: Regex,
}

pub(crate) fn compile(
    position: usize,
    category: &str,
    pattern: &str,
) -> Result<PolicyPattern, PolicyError> {
    let refuse = |reason: &'static str| PolicyError::Pattern {
        position,
        category: category.to_string(),
        reason,
    };
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(refuse("it is over 1 024 bytes"));
    }
    let syntax = regex_syntax::Parser::new()
        .parse(pattern)
        .map_err(|_| refuse("it does not parse"))?;
    let properties = syntax.properties();
    if properties.minimum_len() == Some(0) {
        return Err(refuse("it can match the empty string"));
    }
    match properties.maximum_len() {
        Some(length) if length <= MAX_MATCH_BYTES => {}
        _ => return Err(refuse("its longest match is unbounded or over 256 bytes")),
    }
    let wrapped = format!(r"((?:{pattern}))(?:[^\p{{Alphabetic}}\p{{N}}_]|\z)");
    let regex = RegexBuilder::new(&wrapped)
        .size_limit(COMPILED_SIZE_LIMIT)
        .dfa_size_limit(LAZY_DFA_LIMIT)
        .build()
        .map_err(|_| refuse("it does not compile within the size limits"))?;
    Ok(PolicyPattern {
        category: category.to_string(),
        regex,
    })
}
