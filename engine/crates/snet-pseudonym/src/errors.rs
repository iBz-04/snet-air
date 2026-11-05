use std::path::PathBuf;

/// Why a policy did not load. No variant holds a term, a
/// pattern or any value from the file other than field names.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error("the policy file {path} cannot be read: {cause}")]
    Read { path: PathBuf, cause: String },
    #[error("the policy file is not valid TOML at line {line}: {message}")]
    Syntax { line: usize, message: String },
    #[error("the policy field `{field}` is missing")]
    Missing { field: String },
    #[error("the policy field `{field}` is not allowed")]
    Unknown { field: String },
    #[error("the policy field `{field}` is invalid: {reason}")]
    Invalid { field: String, reason: String },
    #[error("the dictionary {path} is refused: {reason}")]
    Dictionary { path: PathBuf, reason: String },
    #[error("the dictionary {path} is refused at line {line}: {reason}")]
    DictionaryLine {
        path: PathBuf,
        line: usize,
        reason: &'static str,
    },
    #[error("policy pattern {position} of category {category} is refused: {reason}")]
    Pattern {
        position: usize,
        category: String,
        reason: &'static str,
    },
    #[error("the policy exceeds its limit of {limit}")]
    Limit { limit: &'static str },
}

/// Why pseudonymising, building or restoring a text failed. Positions count
/// Unicode scalar values; paths count members and items, never names.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PseudonymError {
    #[error("the text is {size} bytes, over the 1 MiB limit")]
    TextTooLarge { size: usize },
    #[error("the text holds a pseudonym bracket at character {position}")]
    Bracket { position: usize },
    #[error("the task's mapping is full: {limit}")]
    MappingFull { limit: &'static str },
    #[error("the JSON number at {path} looks like a value of category {category}")]
    NumberMatched { path: String, category: String },
    #[error("two keys of the object at {path} became equal")]
    KeysCollide { path: String },
    #[error("the JSON value nests deeper than 64 levels")]
    JsonTooDeep,
    #[error("the JSON value is {size} bytes, over the 1 MiB limit")]
    JsonTooLarge { size: usize },
    #[error("the integer {value} is over 999 999")]
    IntegerOutOfRange { value: u32 },
    #[error("the texts were made for different destinations")]
    DestinationMismatch,
    #[error("the texts were made under different policies")]
    PolicyMismatch,
    #[error("the token at character {position} is not one of this task")]
    TokenUnknown { position: usize },
    #[error("the bracket at character {position} does not open a well-formed token")]
    TokenMalformed { position: usize },
    #[error("a stray pseudonym bracket stands at character {position}")]
    StrayBracket { position: usize },
    #[error("a value of category {category} has no canonical form")]
    NoCanonicalForm { category: String },
    #[error("the serialised mapping is refused: {reason}")]
    MappingBytes { reason: &'static str },
}
