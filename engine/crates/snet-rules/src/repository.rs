use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::commit_messages::check_commit_messages;
use crate::errors::RulesError;
use crate::file_length::{MAXIMUM_COUNTED_LINES, SourceLength, measure_source};
use crate::files::{EntryKind, RepositoryEntry, list_repository_entries, read_entry_bytes};
use crate::forbidden_terms::ForbiddenTerms;
use crate::manifests::{ManifestSource, WORKSPACE_MANIFEST_PATH, check_manifests};
use crate::report::{Finding, Rule};
use crate::test_files::{
    Placement, crate_directories, declares_test_module, is_crate_source,
    module_path_attribute_lines, placement,
};

pub fn check_repository(root: &Path, terms: &ForbiddenTerms) -> Result<Vec<Finding>, RulesError> {
    if !root.is_dir() {
        return Err(RulesError::RootNotDirectory {
            path: root.display().to_string(),
        });
    }

    let mut findings = Vec::new();
    let mut workspace_manifest = None;
    let mut crate_manifests = Vec::new();

    let entries = list_repository_entries(root, terms)?;
    let files: BTreeMap<&str, &RepositoryEntry> = entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::File)
        .map(|entry| (entry.relative_path.as_str(), entry))
        .collect();
    let crates = crate_directories(
        files
            .keys()
            .copied()
            .filter(|path| path.ends_with("/Cargo.toml") && *path != WORKSPACE_MANIFEST_PATH),
    );

    for entry in entries.iter().cloned() {
        let path = terms.display_path(&entry.relative_path);
        let bytes = read_entry_bytes(root, &entry, terms)?;

        if terms.contains_listed_word(entry.relative_path.as_bytes()) {
            findings.push(forbidden_word_finding(&path, 0));
        }
        let file_lines: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
        for line in terms.lines_with_listed_words(&bytes) {
            let allowed = file_lines
                .get(line - 1)
                .is_some_and(|text| terms.is_allowed_line(&entry.relative_path, text));
            if !allowed {
                findings.push(forbidden_word_finding(&path, line));
            }
        }

        if entry.kind == EntryKind::SymbolicLink {
            continue;
        }
        if entry.relative_path.ends_with(".rs") {
            if is_crate_source(&entry.relative_path, &crates) {
                findings.extend(check_module_paths(&path, &bytes)?);
            }
            if is_counted(&entry.relative_path, &crates, &files, root, terms)? {
                findings.extend(check_file_length(&path, &bytes)?);
            }
        }
        if entry.relative_path.rsplit('/').next() == Some("Cargo.toml") {
            let text = String::from_utf8(bytes).map_err(|_| RulesError::ManifestUnparsable {
                path: path.clone(),
                message: "not UTF-8".to_string(),
            })?;
            let manifest = ManifestSource {
                relative_path: entry.relative_path,
                text,
            };
            if manifest.relative_path == WORKSPACE_MANIFEST_PATH {
                workspace_manifest = Some(manifest);
            } else {
                crate_manifests.push(manifest);
            }
        }
    }

    let workspace_manifest = workspace_manifest.ok_or(RulesError::WorkspaceManifestMissing {
        path: WORKSPACE_MANIFEST_PATH.to_string(),
    })?;
    findings.extend(check_manifests(
        &workspace_manifest,
        &crate_manifests,
        terms,
    )?);
    findings.extend(check_commit_messages(root, terms)?);
    Ok(findings)
}

/// Whether the 400-line limit applies: files under a crate's `tests/` are
/// exempt, and a `src/**/tests.rs` only when its parent module declares it
/// under a `cfg` that requires `test`.
fn is_counted(
    relative_path: &str,
    crates: &BTreeSet<String>,
    files: &BTreeMap<&str, &RepositoryEntry>,
    root: &Path,
    terms: &ForbiddenTerms,
) -> Result<bool, RulesError> {
    match placement(relative_path, crates) {
        Placement::CrateTests => Ok(false),
        Placement::Ordinary => Ok(true),
        Placement::SourceTestsFile { declaring_files } => {
            for declaring in &declaring_files {
                let Some(entry) = files.get(declaring.as_str()) else {
                    continue;
                };
                let path = terms.display_path(declaring);
                let bytes = read_entry_bytes(root, entry, terms)?;
                let source = rust_text(&path, &bytes)?;
                let declared =
                    declares_test_module(source).map_err(|error| unparsable(&path, &error))?;
                if declared {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
}

fn check_module_paths(path: &str, bytes: &[u8]) -> Result<Vec<Finding>, RulesError> {
    let source = rust_text(path, bytes)?;
    let lines = module_path_attribute_lines(source).map_err(|error| unparsable(path, &error))?;
    Ok(lines
        .into_iter()
        .map(|line| Finding {
            path: path.to_string(),
            line,
            rule: Rule::ModulePath,
            detail: "a #[path] attribute moves a module away from where snet-rules looks for it"
                .to_string(),
        })
        .collect())
}

fn rust_text<'b>(path: &str, bytes: &'b [u8]) -> Result<&'b str, RulesError> {
    std::str::from_utf8(bytes).map_err(|_| RulesError::RustNotUtf8 {
        path: path.to_string(),
    })
}

fn unparsable(path: &str, error: &syn::Error) -> RulesError {
    RulesError::RustUnparsable {
        path: path.to_string(),
        line: error.span().start().line,
        message: error.to_string(),
    }
}

fn check_file_length(path: &str, bytes: &[u8]) -> Result<Option<Finding>, RulesError> {
    let source = rust_text(path, bytes)?;
    let length = measure_source(source).map_err(|error| unparsable(path, &error))?;
    Ok(match length {
        SourceLength::Counted(lines) if lines > MAXIMUM_COUNTED_LINES => Some(Finding {
            path: path.to_string(),
            line: 0,
            rule: Rule::FileLength,
            detail: format!(
                "{lines} lines outside tests, more than {MAXIMUM_COUNTED_LINES}; split it into a module folder"
            ),
        }),
        SourceLength::Counted(_) | SourceLength::TestFile => None,
    })
}

fn forbidden_word_finding(path: &str, line: usize) -> Finding {
    Finding {
        path: path.to_string(),
        line,
        rule: Rule::ForbiddenWord,
        detail: "holds a word on the forbidden-terms list".to_string(),
    }
}
