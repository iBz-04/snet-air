//! Every commit message reachable from `HEAD`, read as raw commit objects
//! and checked for listed words.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use crate::errors::RulesError;
use crate::forbidden_terms::ForbiddenTerms;
use crate::report::{Finding, Rule};

const SHOWN_HASH_LENGTH: usize = 12;

pub fn check_commit_messages(
    root: &Path,
    terms: &ForbiddenTerms,
) -> Result<Vec<Finding>, RulesError> {
    let shallow = git(
        root,
        "shallow check",
        &["rev-parse", "--is-shallow-repository"],
    )?;
    if trimmed(&shallow.stdout) == "true" {
        return Err(RulesError::ShallowClone {
            root: root.display().to_string(),
        });
    }
    let head = run_git(root, "HEAD", &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    if !head.status.success() {
        return unborn_or_broken(root, &head);
    }
    let listed = git(root, "commit list", &["rev-list", "HEAD"])?;
    let hashes: Vec<String> = trimmed(&listed.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    let objects = read_commit_objects(root, &hashes)?;

    let mut findings = Vec::new();
    for (hash, bytes) in &objects {
        findings.extend(check_commit(hash, bytes, terms));
    }
    Ok(findings)
}

/// `HEAD` resolves to nothing: that is a repository without commits only
/// when no ref exists at all and git says nothing about broken ones
///.
fn unborn_or_broken(root: &Path, head: &Output) -> Result<Vec<Finding>, RulesError> {
    let refs = run_git(root, "ref list", &["for-each-ref"])?;
    let unborn = head.status.code() == Some(1)
        && head.stdout.is_empty()
        && refs.status.success()
        && refs.stdout.is_empty()
        && refs.stderr.is_empty();
    if unborn {
        return Ok(Vec::new());
    }
    Err(history_error(
        root,
        "HEAD",
        "HEAD does not name a commit, yet the repository has refs or reports a broken one",
    ))
}

fn check_commit(hash: &str, bytes: &[u8], terms: &ForbiddenTerms) -> Vec<Finding> {
    let label = format!("commit {}", &hash[..SHOWN_HASH_LENGTH.min(hash.len())]);
    let finding = |line: usize, rule: Rule, detail: &str| Finding {
        path: label.clone(),
        line,
        rule,
        detail: detail.to_string(),
    };
    let mut findings = Vec::new();
    if bytes.contains(&0) {
        findings.push(finding(
            0,
            Rule::CommitObject,
            "the commit holds a NUL byte, which git log would cut the message at",
        ));
    }
    let Some(separator) = bytes.windows(2).position(|pair| pair == b"\n\n") else {
        findings.push(finding(
            0,
            Rule::CommitObject,
            "the commit has no blank line after its header, so it has no message",
        ));
        return findings;
    };
    let (header, message) = (&bytes[..separator], &bytes[separator + 2..]);
    let foreign_encoding = header.split(|byte| *byte == b'\n').any(|line| {
        line.strip_prefix(b"encoding ")
            .is_some_and(|encoding| !encoding.eq_ignore_ascii_case(b"utf-8"))
    });
    if foreign_encoding {
        findings.push(finding(
            0,
            Rule::CommitObject,
            "the commit names an encoding other than UTF-8, which git log would re-code",
        ));
    }
    for line in terms.lines_with_listed_words(message) {
        findings.push(finding(
            line,
            Rule::ForbiddenWord,
            "the message holds a word on the forbidden-terms list",
        ));
    }
    findings
}

/// Reads each commit's exact bytes through one `git cat-file --batch`. A
/// thread writes the hashes while this one reads, so neither pipe can fill
/// and stall the other.
fn read_commit_objects(
    root: &Path,
    hashes: &[String],
) -> Result<Vec<(String, Vec<u8>)>, RulesError> {
    let mut child = git_command(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| history_error(root, "object read", &error.to_string()))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| history_error(root, "object read", "git's standard input is closed"))?;
    let input: Vec<u8> = hashes
        .iter()
        .flat_map(|hash| format!("{hash}\n").into_bytes())
        .collect();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child
        .wait_with_output()
        .map_err(|error| history_error(root, "object read", &error.to_string()))?;
    writer
        .join()
        .map_err(|_| history_error(root, "object read", "the writer thread panicked"))?
        .map_err(|error| history_error(root, "object read", &error.to_string()))?;
    if !output.status.success() {
        return Err(history_error(
            root,
            "object read",
            &String::from_utf8_lossy(&output.stderr),
        ));
    }
    parse_batch(&output.stdout).map_err(|detail| history_error(root, "object read", &detail))
}

/// Splits `cat-file --batch` output: `<oid> commit <size>\n`, exactly
/// `size` bytes, `\n`. Any other reply, such as `<oid> missing`, is an error.
pub fn parse_batch(output: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut objects = Vec::new();
    let mut rest = output;
    while !rest.is_empty() {
        let line_end = rest
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or("a reply line has no end")?;
        let line =
            std::str::from_utf8(&rest[..line_end]).map_err(|_| "a reply line is not UTF-8")?;
        let fields: Vec<&str> = line.split(' ').collect();
        let [hash, "commit", size] = fields.as_slice() else {
            return Err(format!("git did not return a commit: `{line}`"));
        };
        let size: usize = size
            .parse()
            .map_err(|_| format!("a reply names a bad size: `{line}`"))?;
        let body_start = line_end + 1;
        let body_end = body_start
            .checked_add(size)
            .filter(|end| *end < rest.len() && rest[*end] == b'\n')
            .ok_or_else(|| format!("the object of `{line}` is cut short"))?;
        objects.push((hash.to_string(), rest[body_start..body_end].to_vec()));
        rest = &rest[body_end + 1..];
    }
    Ok(objects)
}

fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .args(["--no-replace-objects", "-c", "log.showSignature=false"]);
    command
}

fn run_git(root: &Path, step: &'static str, arguments: &[&str]) -> Result<Output, RulesError> {
    git_command(root)
        .args(arguments)
        .output()
        .map_err(|error| history_error(root, step, &error.to_string()))
}

fn git(root: &Path, step: &'static str, arguments: &[&str]) -> Result<Output, RulesError> {
    let output = run_git(root, step, arguments)?;
    if !output.status.success() {
        return Err(history_error(
            root,
            step,
            &String::from_utf8_lossy(&output.stderr),
        ));
    }
    Ok(output)
}

fn history_error(root: &Path, step: &'static str, detail: &str) -> RulesError {
    RulesError::GitHistory {
        root: root.display().to_string(),
        step,
        detail: detail.trim().to_string(),
    }
}

fn trimmed(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}
