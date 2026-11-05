use snet_rules::errors::RulesError;
use snet_rules::report::Rule;
use snet_rules::repository::check_repository;

use crate::support::{
    PLANTED_WORD, TestResult, commit_all, commit_raw, git_output, passing_repository,
    terms_listing, write_file,
};

fn commit_findings(root: &std::path::Path) -> TestResult<Vec<(String, usize, Rule)>> {
    let terms = terms_listing(&[PLANTED_WORD])?;
    Ok(check_repository(root, &terms)?
        .into_iter()
        .filter(|finding| finding.path.starts_with("commit "))
        .map(|finding| (finding.path, finding.line, finding.rule))
        .collect())
}

fn label(hash: &str) -> String {
    format!("commit {}", &hash[..12])
}

#[test]
fn a_repository_without_commits_passes() {
    let repository = passing_repository().unwrap();
    assert!(commit_findings(repository.path()).unwrap().is_empty());
}

#[test]
fn a_clean_history_with_a_merge_passes() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    let first = commit_all(root, "add: first change").unwrap();
    write_file(root, "docs/side.md", b"side\n").unwrap();
    let side = commit_all(root, "add: side change").unwrap();
    let tree = git_output(root, &["write-tree"], b"").unwrap();
    let merge = git_output(
        root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &first,
            "-p",
            &side,
            "-m",
            "add: merged work",
        ],
        b"",
    )
    .unwrap();
    git_output(root, &["update-ref", "HEAD", &merge], b"").unwrap();
    assert!(commit_findings(root).unwrap().is_empty());
}

#[test]
fn a_listed_word_in_a_message_is_reported_with_its_hash_and_line_but_not_the_word() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    commit_all(root, "add: clean start").unwrap();
    write_file(root, "docs/notes.md", b"notes\n").unwrap();
    let hash = commit_all(root, "add: notes\n\nThe PlantedName client is gone.").unwrap();
    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let findings = check_repository(root, &terms).unwrap();
    let printed: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert_eq!(
        printed,
        vec![format!(
            "{}:3: forbidden-word: the message holds a word on the forbidden-terms list",
            label(&hash)
        )]
    );
    assert!(
        printed
            .iter()
            .all(|line| !line.to_lowercase().contains(PLANTED_WORD))
    );
}

#[test]
fn a_nul_byte_hides_nothing() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    let hash = commit_raw(root, "", "\n", b"add: visible\0plantedname after the nul\n").unwrap();
    assert_eq!(
        commit_findings(root).unwrap(),
        vec![
            (label(&hash), 0, Rule::CommitObject),
            (label(&hash), 1, Rule::ForbiddenWord),
        ]
    );
}

#[test]
fn a_foreign_encoding_is_reported_and_utf8_in_any_case_is_not() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    let foreign = commit_raw(root, "encoding IBM037\n", "\n", b"add: coded\n").unwrap();
    assert_eq!(
        commit_findings(root).unwrap(),
        vec![(label(&foreign), 0, Rule::CommitObject)]
    );
    let other = passing_repository().unwrap();
    commit_raw(other.path(), "encoding utf-8\n", "\n", b"add: plain\n").unwrap();
    assert!(commit_findings(other.path()).unwrap().is_empty());
}

#[test]
fn a_commit_without_a_blank_line_after_its_header_is_reported() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    let hash = commit_raw(root, "", "", b"").unwrap();
    assert_eq!(
        commit_findings(root).unwrap(),
        vec![(label(&hash), 0, Rule::CommitObject)]
    );
}

#[test]
fn a_branch_ref_holding_garbage_stops_the_run() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    let branch = git_output(root, &["symbolic-ref", "HEAD"], b"").unwrap();
    write_file(root, &format!(".git/{branch}"), b"garbage\n").unwrap();
    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let result = check_repository(root, &terms);
    assert!(
        matches!(result, Err(RulesError::GitHistory { .. })),
        "{result:?}"
    );
}

#[test]
fn an_unborn_head_beside_other_commits_stops_the_run() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    commit_all(root, "add: on the first branch").unwrap();
    git_output(root, &["symbolic-ref", "HEAD", "refs/heads/elsewhere"], b"").unwrap();
    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let result = check_repository(root, &terms);
    assert!(
        matches!(result, Err(RulesError::GitHistory { .. })),
        "{result:?}"
    );
}

#[test]
fn a_shallow_clone_stops_the_run() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    commit_all(root, "add: first").unwrap();
    write_file(root, "docs/second.md", b"second\n").unwrap();
    commit_all(root, "add: second").unwrap();
    let clone = tempfile::tempdir().unwrap();
    let source = format!("file://{}", root.display());
    let destination = clone.path().join("shallow");
    git_output(
        clone.path(),
        &[
            "clone",
            "-q",
            "--depth",
            "1",
            &source,
            destination.to_str().unwrap(),
        ],
        b"",
    )
    .unwrap();
    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let result = check_repository(&destination, &terms);
    assert!(
        matches!(result, Err(RulesError::ShallowClone { .. })),
        "{result:?}"
    );
}
