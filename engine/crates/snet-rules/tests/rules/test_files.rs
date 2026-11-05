use std::collections::BTreeSet;

use snet_rules::test_files::{
    Placement, crate_directories, declares_test_module, is_crate_source,
    module_path_attribute_lines, placement,
};

fn crates() -> BTreeSet<String> {
    crate_directories([
        "engine/crates/snet-x/Cargo.toml",
        "engine/crates/snet-y/Cargo.toml",
    ])
}

#[test]
fn files_under_a_crates_tests_directory_are_exempt_by_place() {
    assert_eq!(
        placement("engine/crates/snet-x/tests/flow.rs", &crates()),
        Placement::CrateTests
    );
    assert_eq!(
        placement("engine/crates/snet-x/tests/cloud/support/mod.rs", &crates()),
        Placement::CrateTests
    );
}

#[test]
fn a_tests_directory_inside_src_or_outside_a_crate_gives_no_exemption() {
    assert_eq!(
        placement("engine/crates/snet-x/src/plan/tests/helpers.rs", &crates()),
        Placement::Ordinary
    );
    assert_eq!(
        placement("tools/tests/script.rs", &crates()),
        Placement::Ordinary
    );
    assert_eq!(
        placement("engine/crates/snet-x/src/testsuite.rs", &crates()),
        Placement::Ordinary
    );
}

#[test]
fn a_source_tests_file_names_its_possible_declaring_files() {
    assert_eq!(
        placement("engine/crates/snet-x/src/plan/tests.rs", &crates()),
        Placement::SourceTestsFile {
            declaring_files: vec![
                "engine/crates/snet-x/src/plan.rs".to_string(),
                "engine/crates/snet-x/src/plan/mod.rs".to_string(),
            ]
        }
    );
    assert_eq!(
        placement("engine/crates/snet-x/src/tests.rs", &crates()),
        Placement::SourceTestsFile {
            declaring_files: vec![
                "engine/crates/snet-x/src/lib.rs".to_string(),
                "engine/crates/snet-x/src/main.rs".to_string(),
            ]
        }
    );
    assert_eq!(
        placement("engine/crates/snet-y/src/a/b/tests.rs", &crates()),
        Placement::SourceTestsFile {
            declaring_files: vec![
                "engine/crates/snet-y/src/a/b.rs".to_string(),
                "engine/crates/snet-y/src/a/b/mod.rs".to_string(),
            ]
        }
    );
}

#[test]
fn only_a_cfg_requiring_test_on_the_declaration_counts() {
    assert!(declares_test_module("#[cfg(test)]\nmod tests;\n").unwrap());
    assert!(declares_test_module("#[cfg(all(test, unix))]\nmod tests;\n").unwrap());
    assert!(declares_test_module("#[cfg(all(unix, test))]\nmod tests;\n").unwrap());
    assert!(!declares_test_module("mod tests;\n").unwrap());
    assert!(!declares_test_module("#[cfg(any(test, unix))]\nmod tests;\n").unwrap());
    assert!(!declares_test_module("#[cfg_attr(test, allow(dead_code))]\nmod tests;\n").unwrap());
    assert!(!declares_test_module("#[cfg(all(not(test), unix))]\nmod tests;\n").unwrap());
    assert!(!declares_test_module("#[test]\nmod tests;\n").unwrap());
    assert!(!declares_test_module("#[cfg(test)]\nmod outer {\n    mod tests;\n}\n").unwrap());
    assert!(!declares_test_module("#[cfg(test)]\nmod tests {}\n").unwrap());
    assert!(!declares_test_module("#[cfg(test)]\nmod other;\n").unwrap());
}

#[test]
fn path_attributes_are_found_at_any_depth() {
    let source = "#[path = \"a.rs\"]\nmod a;\nmod outer {\n    #[path = \"b.rs\"]\n    mod b;\n}\nmod plain;\n";
    assert_eq!(module_path_attribute_lines(source).unwrap(), vec![1, 4]);
}

#[test]
fn crate_sources_are_the_files_under_a_crates_src() {
    assert!(is_crate_source(
        "engine/crates/snet-x/src/lib.rs",
        &crates()
    ));
    assert!(!is_crate_source(
        "engine/crates/snet-x/tests/flow.rs",
        &crates()
    ));
    assert!(!is_crate_source("engine/crates/snet-x/build.rs", &crates()));
    assert!(!is_crate_source("tools/src/lib.rs", &crates()));
}
