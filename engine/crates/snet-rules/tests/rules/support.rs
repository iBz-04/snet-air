use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};
use snet_rules::forbidden_terms::ForbiddenTerms;
use tempfile::TempDir;

pub type TestResult<T> = Result<T, Box<dyn Error>>;

pub const PLANTED_WORD: &str = "plantedname";

pub fn digest_hex(word: &str) -> TestResult<String> {
    let mut hex = String::new();
    for byte in Sha256::digest(word.as_bytes()) {
        write!(hex, "{byte:02x}")?;
    }
    Ok(hex)
}

pub fn terms_listing(words: &[&str]) -> TestResult<ForbiddenTerms> {
    let mut list = String::new();
    for word in words {
        list.push_str(&digest_hex(word)?);
        list.push('\n');
    }
    Ok(ForbiddenTerms::parse(&list)?)
}

pub fn write_file(root: &Path, relative_path: &str, contents: &[u8]) -> TestResult<()> {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)?;
    Ok(())
}

pub fn git(root: &Path, arguments: &[&str]) -> TestResult<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .status()?;
    if !status.success() {
        return Err(format!("git {arguments:?} failed with {status}").into());
    }
    Ok(())
}

pub const WORKSPACE_MANIFEST: &str = r#"[workspace]
members = ["crates/snet-sample"]
resolver = "3"

[workspace.package]
license = "MPL-2.0"
publish = false
"#;

pub const SAMPLE_CRATE_MANIFEST: &str = r#"[package]
name = "snet-sample"
license.workspace = true
publish.workspace = true

[lints]
workspace = true
"#;

/// A git repository holding a workspace that passes every rule.
pub fn passing_repository() -> TestResult<TempDir> {
    let directory = tempfile::tempdir()?;
    git(directory.path(), &["init", "-q"])?;
    write_file(
        directory.path(),
        "engine/Cargo.toml",
        WORKSPACE_MANIFEST.as_bytes(),
    )?;
    write_file(
        directory.path(),
        "engine/crates/snet-sample/Cargo.toml",
        SAMPLE_CRATE_MANIFEST.as_bytes(),
    )?;
    write_file(
        directory.path(),
        "engine/crates/snet-sample/src/lib.rs",
        b"pub fn sample() {}\n",
    )?;
    Ok(directory)
}

pub fn lines_of_code(count: usize) -> String {
    (0..count)
        .map(|index| format!("const C{index}: u8 = 0;\n"))
        .collect()
}

/// Runs git with a fixed identity and no signing, feeding `input` on
/// standard input, and returns standard output without its trailing newline.
pub fn git_output(root: &Path, arguments: &[&str], input: &[u8]) -> TestResult<String> {
    use std::io::Write as _;
    use std::process::Stdio;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("git's standard input is closed")?
        .write_all(input)?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!(
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim_end().to_string())
}

/// Commits everything in the working tree with `message`.
pub fn commit_all(root: &Path, message: &str) -> TestResult<String> {
    git_output(root, &["add", "-A"], b"")?;
    git_output(root, &["commit", "-q", "-m", message], b"")?;
    git_output(root, &["rev-parse", "HEAD"], b"")
}

/// Writes a commit object exactly as given, on top of the current index's
/// tree, and points `HEAD`'s branch at it.
pub fn commit_raw(
    root: &Path,
    header_extra: &str,
    separator: &str,
    message: &[u8],
) -> TestResult<String> {
    git_output(root, &["add", "-A"], b"")?;
    let tree = git_output(root, &["write-tree"], b"")?;
    let mut object = format!(
        "tree {tree}\nauthor Test <test@example.invalid> 0 +0000\ncommitter Test <test@example.invalid> 0 +0000\n{header_extra}{separator}"
    )
    .into_bytes();
    object.extend_from_slice(message);
    let hash = git_output(
        root,
        &[
            "hash-object",
            "-t",
            "commit",
            "--literally",
            "-w",
            "--stdin",
        ],
        &object,
    )?;
    git_output(root, &["update-ref", "HEAD", &hash], b"")?;
    Ok(hash)
}
