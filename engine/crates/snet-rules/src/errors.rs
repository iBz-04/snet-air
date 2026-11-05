use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RulesError {
    #[error("usage: snet-rules <repository root>")]
    Usage,

    #[error("{path}: the repository root is not a directory")]
    RootNotDirectory { path: String },

    #[error("{root}: git could not be started to list the repository's files: {source}")]
    GitNotRunnable { root: String, source: io::Error },

    #[error("{root}: git could not list the repository's files: {stderr}")]
    GitListingFailed { root: String, stderr: String },

    #[error("{root}: git could not read the commit history ({step}): {detail}")]
    GitHistory {
        root: String,
        step: &'static str,
        detail: String,
    },

    #[error(
        "{root}: the repository is a shallow clone, so its commit history is incomplete; fetch the full history"
    )]
    ShallowClone { root: String },

    #[error("{root}: git listed a path that is not UTF-8")]
    PathNotUtf8 { root: String },

    #[error("{path}: listed by git but could not be read: {source}")]
    FileUnreadable { path: String, source: io::Error },

    #[error("{path}: listed by git but is a directory; submodules are not allowed")]
    SubmoduleListed { path: String },

    #[error("{path}: Rust source is not UTF-8")]
    RustNotUtf8 { path: String },

    #[error("{path}:{line}: Rust source does not parse: {message}")]
    RustUnparsable {
        path: String,
        line: usize,
        message: String,
    },

    #[error("{path}: manifest cannot be read as TOML: {message}")]
    ManifestUnparsable { path: String, message: String },

    #[error("{path}: the workspace manifest is not among the repository files")]
    WorkspaceManifestMissing { path: String },

    #[error("the forbidden-terms list is empty")]
    ForbiddenTermsEmpty,

    #[error("the forbidden-terms list line {line} is not 64 lowercase hex characters")]
    ForbiddenTermMalformed { line: usize },
}
