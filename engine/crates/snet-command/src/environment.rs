use std::path::PathBuf;

use crate::exit::CommandError;

/// What the command reads from its surroundings besides its arguments, so
/// tests can give it a fixed one.
pub trait Environment {
    /// The variable's value, or `None` when it is unset.
    fn variable(&self, name: &str) -> Result<Option<String>, CommandError>;
    fn home_directory(&self) -> Option<PathBuf>;
}

pub struct ProcessEnvironment;

impl Environment for ProcessEnvironment {
    fn variable(&self, name: &str) -> Result<Option<String>, CommandError> {
        match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(CommandError::configuration(format!(
                "the environment variable {name} is not UTF-8"
            ))),
        }
    }

    fn home_directory(&self) -> Option<PathBuf> {
        std::env::home_dir()
    }
}
