use std::collections::HashMap;
use std::path::PathBuf;

use crate::environment::Environment;
use crate::exit::CommandError;

/// An environment with fixed variables and home directory.
pub(crate) struct FixedEnvironment {
    pub(crate) variables: HashMap<String, String>,
    pub(crate) home: Option<PathBuf>,
}

impl FixedEnvironment {
    pub(crate) fn new(variables: &[(&str, &str)]) -> Self {
        Self {
            variables: variables
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            home: Some(PathBuf::from("/home/someone")),
        }
    }
}

impl Environment for FixedEnvironment {
    fn variable(&self, name: &str) -> Result<Option<String>, CommandError> {
        Ok(self.variables.get(name).cloned())
    }

    fn home_directory(&self) -> Option<PathBuf> {
        self.home.clone()
    }
}
