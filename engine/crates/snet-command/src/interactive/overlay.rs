use std::collections::HashMap;
use std::path::PathBuf;

use crate::environment::Environment;
use crate::exit::CommandError;

/// The process environment with the values typed in this session laid over
/// it, such as a key pasted at the prompt. Nothing here is written anywhere.
pub struct OverlayEnvironment<'a> {
    base: &'a dyn Environment,
    values: HashMap<String, String>,
}

impl<'a> OverlayEnvironment<'a> {
    pub fn new(base: &'a dyn Environment) -> Self {
        Self {
            base,
            values: HashMap::new(),
        }
    }

    pub fn set(&mut self, name: &str, value: String) {
        self.values.insert(name.to_string(), value);
    }
}

impl Environment for OverlayEnvironment<'_> {
    fn variable(&self, name: &str) -> Result<Option<String>, CommandError> {
        match self.values.get(name) {
            Some(value) => Ok(Some(value.clone())),
            None => self.base.variable(name),
        }
    }

    fn home_directory(&self) -> Option<PathBuf> {
        self.base.home_directory()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FixedEnvironment;

    #[test]
    fn typed_values_win_and_the_rest_comes_from_the_process() {
        let base = FixedEnvironment::new(&[("SNET_HOME", "/h"), ("MELIOUS_API_KEY", "old")]);
        let mut overlay = OverlayEnvironment::new(&base);
        overlay.set("MELIOUS_API_KEY", "typed".to_string());
        assert_eq!(
            overlay.variable("MELIOUS_API_KEY").unwrap().as_deref(),
            Some("typed")
        );
        assert_eq!(
            overlay.variable("SNET_HOME").unwrap().as_deref(),
            Some("/h")
        );
        assert_eq!(overlay.variable("OLLAMA_HOST").unwrap(), None);
    }
}
