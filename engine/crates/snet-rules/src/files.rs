use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::errors::RulesError;
use crate::forbidden_terms::ForbiddenTerms;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    SymbolicLink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryEntry {
    /// The `/`-separated path git printed, relative to the repository root.
    pub relative_path: String,
    pub kind: EntryKind,
}

/// Every tracked file, including force-added ones, and every untracked file
/// git does not ignore.
pub fn list_repository_entries(
    root: &Path,
    terms: &ForbiddenTerms,
) -> Result<Vec<RepositoryEntry>, RulesError> {
    let root_display = root.display().to_string();
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()
        .map_err(|source| RulesError::GitNotRunnable {
            root: root_display.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(RulesError::GitListingFailed {
            root: root_display,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    let mut relative_paths = BTreeSet::new();
    for raw_path in output.stdout.split(|&byte| byte == 0) {
        if raw_path.is_empty() {
            continue;
        }
        let relative_path =
            String::from_utf8(raw_path.to_vec()).map_err(|_| RulesError::PathNotUtf8 {
                root: root_display.clone(),
            })?;
        relative_paths.insert(relative_path);
    }

    relative_paths
        .into_iter()
        .map(|relative_path| classify_entry(root, relative_path, terms))
        .collect()
}

fn classify_entry(
    root: &Path,
    relative_path: String,
    terms: &ForbiddenTerms,
) -> Result<RepositoryEntry, RulesError> {
    let metadata = fs::symlink_metadata(root.join(&relative_path)).map_err(|source| {
        RulesError::FileUnreadable {
            path: terms.display_path(&relative_path),
            source,
        }
    })?;
    let kind = if metadata.is_symlink() {
        EntryKind::SymbolicLink
    } else if metadata.is_dir() {
        return Err(RulesError::SubmoduleListed {
            path: terms.display_path(&relative_path),
        });
    } else {
        EntryKind::File
    };
    Ok(RepositoryEntry {
        relative_path,
        kind,
    })
}

/// A symbolic link yields its link text and is never followed.
pub fn read_entry_bytes(
    root: &Path,
    entry: &RepositoryEntry,
    terms: &ForbiddenTerms,
) -> Result<Vec<u8>, RulesError> {
    let full_path = root.join(&entry.relative_path);
    let unreadable = |source| RulesError::FileUnreadable {
        path: terms.display_path(&entry.relative_path),
        source,
    };
    match entry.kind {
        EntryKind::File => fs::read(&full_path).map_err(unreadable),
        EntryKind::SymbolicLink => fs::read_link(&full_path)
            .map(|target| target.into_os_string().into_encoded_bytes())
            .map_err(unreadable),
    }
}
