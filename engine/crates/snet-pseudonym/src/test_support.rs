#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::outbound::Destination;
use crate::policy::Policy;

/// A private directory for dictionary files, removed when dropped.
pub(crate) struct Fixture {
    pub(crate) directory: tempfile::TempDir,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), PermissionsExt::from_mode(0o700)).unwrap();
        Self { directory }
    }

    pub(crate) fn dictionary(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.directory.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, PermissionsExt::from_mode(0o600)).unwrap();
        path
    }

    /// A policy with every built-in on, a `PERSON` dictionary of `names`,
    /// and the given `[[patterns]]` and `[[passthrough]]` tables (empty
    /// strings for none).
    pub(crate) fn policy(&self, names: &[&str], patterns: &str, passthrough: &str) -> Policy {
        let path = self.dictionary("people.txt", format!("{}\n", names.join("\n")).as_bytes());
        Policy::parse(&policy_text(&path, patterns, passthrough)).unwrap()
    }
}

pub(crate) const ALL_BUILTINS: &str = "[builtin]\nEMAIL = true\nPHONE = true\nIBAN = true\nPAYMENT_CARD = true\nTR_IDENTITY = true\nIP_ADDRESS = true\nMAC_ADDRESS = true\n";

/// Top-level empty arrays come before every table header, since TOML puts
/// a key written after a header inside that table.
pub(crate) fn policy_text(
    dictionary: &std::path::Path,
    patterns: &str,
    passthrough: &str,
) -> String {
    let mut text = String::new();
    if patterns.is_empty() {
        text.push_str("patterns = []\n");
    }
    if passthrough.is_empty() {
        text.push_str("passthrough = []\n");
    }
    text.push_str(ALL_BUILTINS);
    text.push_str(&format!(
        "\n[[dictionaries]]\ncategory = \"PERSON\"\npath = \"{}\"\n\n{patterns}\n{passthrough}\n",
        dictionary.display()
    ));
    text
}

pub(crate) fn cloud() -> Destination {
    Destination::new("main", "api.example.com")
}
