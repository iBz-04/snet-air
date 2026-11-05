use std::fs;
use std::path::Path;

use crate::support::{
    PLANTED_WORD, git, lines_of_code, passing_repository, terms_listing, write_file,
};
use snet_rules::errors::RulesError;
use snet_rules::report::Rule;
use snet_rules::repository::check_repository;

#[test]
fn a_conforming_repository_passes() {
    let repository = passing_repository().unwrap();
    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let findings = check_repository(repository.path(), &terms).unwrap();
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn listed_words_in_contents_names_and_force_added_files_are_reported_without_the_word() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    write_file(
        root,
        "docs/notes.md",
        b"first line\nthe PlantedName client\n",
    )
    .unwrap();
    write_file(root, "docs/plantedname_guide.md", b"clean\n").unwrap();
    write_file(root, ".gitignore", b"/ignored/\n").unwrap();
    write_file(root, "ignored/forced.txt", b"plantedname\n").unwrap();
    write_file(root, "ignored/untracked.txt", b"plantedname\n").unwrap();
    git(root, &["add", "-f", "ignored/forced.txt"]).unwrap();

    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let findings = check_repository(root, &terms).unwrap();
    let printed: Vec<String> = findings.iter().map(ToString::to_string).collect();

    assert_eq!(
        printed,
        vec![
            "docs/notes.md:2: forbidden-word: holds a word on the forbidden-terms list",
            "docs/<listed:2>:0: forbidden-word: holds a word on the forbidden-terms list",
            "ignored/forced.txt:1: forbidden-word: holds a word on the forbidden-terms list",
        ]
    );
    assert!(
        printed
            .iter()
            .all(|line| !line.to_lowercase().contains(PLANTED_WORD))
    );
}

#[test]
fn a_long_source_file_is_reported_and_test_files_are_exempt() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    let crate_root = "engine/crates/snet-sample";
    let long = lines_of_code(450);
    let files: [(String, String); 12] = [
        (format!("{crate_root}/src/long.rs"), lines_of_code(401)),
        (format!("{crate_root}/src/limit.rs"), lines_of_code(400)),
        (
            format!("{crate_root}/src/plan.rs"),
            "#[cfg(test)]\nmod tests;\n".to_string(),
        ),
        (format!("{crate_root}/src/plan/tests.rs"), long.clone()),
        (
            format!("{crate_root}/src/both/mod.rs"),
            "#[cfg(all(test, unix))]\nmod tests;\n".to_string(),
        ),
        (format!("{crate_root}/src/both/tests.rs"), long.clone()),
        (
            format!("{crate_root}/src/plain.rs"),
            "mod tests;\n".to_string(),
        ),
        (format!("{crate_root}/src/plain/tests.rs"), long.clone()),
        (format!("{crate_root}/src/orphan/tests.rs"), long.clone()),
        (
            format!("{crate_root}/src/nested/tests/helpers.rs"),
            long.clone(),
        ),
        (format!("{crate_root}/tests/flow.rs"), lines_of_code(900)),
        ("tools/script.rs".to_string(), lines_of_code(401)),
    ];
    for (path, contents) in &files {
        write_file(root, path, contents.as_bytes()).unwrap();
    }

    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let findings = check_repository(root, &terms).unwrap();
    let reported: Vec<(&str, Rule)> = findings
        .iter()
        .map(|finding| (finding.path.as_str(), finding.rule))
        .collect();
    assert_eq!(
        reported,
        vec![
            ("engine/crates/snet-sample/src/long.rs", Rule::FileLength),
            (
                "engine/crates/snet-sample/src/nested/tests/helpers.rs",
                Rule::FileLength
            ),
            (
                "engine/crates/snet-sample/src/orphan/tests.rs",
                Rule::FileLength
            ),
            (
                "engine/crates/snet-sample/src/plain/tests.rs",
                Rule::FileLength
            ),
            ("tools/script.rs", Rule::FileLength),
        ]
    );
}

#[test]
fn a_module_path_attribute_fails_the_check() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    write_file(
        root,
        "engine/crates/snet-sample/src/routed.rs",
        b"#[cfg(test)]\n#[path = \"elsewhere.rs\"]\nmod tests;\n",
    )
    .unwrap();
    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let findings = check_repository(root, &terms).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, Rule::ModulePath);
    assert_eq!(findings[0].line, 2);
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_is_checked_by_its_link_text() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    std::os::unix::fs::symlink("missing/plantedname.md", root.join("link.md")).unwrap();

    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let findings = check_repository(root, &terms).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].path, "link.md");
    assert_eq!(findings[0].line, 1);
}

#[test]
fn a_missing_root_is_an_error() {
    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let result = check_repository(Path::new("/nonexistent/snet-rules-root"), &terms);
    assert!(matches!(result, Err(RulesError::RootNotDirectory { .. })));
}

#[test]
fn a_directory_that_is_not_a_git_repository_is_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let result = check_repository(directory.path(), &terms);
    assert!(matches!(result, Err(RulesError::GitListingFailed { .. })));
}

#[test]
fn a_tracked_file_deleted_from_disk_is_an_error() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    write_file(root, "docs/gone.md", b"text\n").unwrap();
    git(root, &["add", "docs/gone.md"]).unwrap();
    fs::remove_file(root.join("docs/gone.md")).unwrap();

    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let result = check_repository(root, &terms);
    assert!(
        matches!(result, Err(RulesError::FileUnreadable { ref path, .. }) if path == "docs/gone.md")
    );
}

#[test]
fn rust_source_that_is_not_utf8_is_an_error() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    write_file(
        root,
        "engine/crates/snet-sample/src/bytes.rs",
        &[0xff, 0xfe, b'\n'],
    )
    .unwrap();

    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let result = check_repository(root, &terms);
    assert!(matches!(result, Err(RulesError::RustNotUtf8 { .. })));
}

#[test]
fn unparsable_rust_source_is_an_error_with_its_line() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    write_file(
        root,
        "engine/crates/snet-sample/src/broken.rs",
        b"fn fine() {}\nfn broken( {\n",
    )
    .unwrap();

    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let result = check_repository(root, &terms);
    assert!(matches!(result, Err(RulesError::RustUnparsable { line, .. }) if line >= 2));
}

#[test]
fn an_unparsable_manifest_is_an_error() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    write_file(root, "engine/crates/snet-sample/Cargo.toml", b"[package\n").unwrap();

    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let result = check_repository(root, &terms);
    assert!(matches!(result, Err(RulesError::ManifestUnparsable { .. })));
}

#[test]
fn a_repository_without_the_workspace_manifest_is_an_error() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    fs::remove_file(root.join("engine/Cargo.toml")).unwrap();

    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let result = check_repository(root, &terms);
    assert!(matches!(
        result,
        Err(RulesError::WorkspaceManifestMissing { .. })
    ));
}

#[test]
fn a_submodule_entry_is_an_error() {
    let repository = passing_repository().unwrap();
    let root = repository.path();
    fs::create_dir_all(root.join("vendored")).unwrap();
    git(
        root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            "160000,1111111111111111111111111111111111111111,vendored",
        ],
    )
    .unwrap();

    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let result = check_repository(root, &terms);
    assert!(matches!(result, Err(RulesError::SubmoduleListed { ref path }) if path == "vendored"));
}
