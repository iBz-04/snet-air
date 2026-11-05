use crate::support::lines_of_code;
use snet_rules::file_length::{SourceLength, measure_source};

#[test]
fn counts_every_line_of_plain_code() {
    assert_eq!(
        measure_source(&lines_of_code(400)).unwrap(),
        SourceLength::Counted(400)
    );
    assert_eq!(
        measure_source(&lines_of_code(401)).unwrap(),
        SourceLength::Counted(401)
    );
}

#[test]
fn inline_test_module_is_not_counted() {
    let source = format!(
        "{}#[cfg(test)]\nmod tests {{\n    #[test]\n    fn passes() {{}}\n}}\n",
        lines_of_code(10)
    );
    assert_eq!(measure_source(&source).unwrap(), SourceLength::Counted(10));
}

#[test]
fn doc_comment_on_a_test_item_is_part_of_its_span() {
    let source = format!(
        "{}/// Tests.\n#[cfg(test)]\nmod tests {{}}\n",
        lines_of_code(3)
    );
    assert_eq!(measure_source(&source).unwrap(), SourceLength::Counted(3));
}

#[test]
fn nested_test_items_are_not_counted_and_not_subtracted_twice() {
    let source = "mod outer {\n    #[cfg(test)]\n    mod inner {\n        #[test]\n        fn passes() {}\n    }\n}\n";
    assert_eq!(measure_source(source).unwrap(), SourceLength::Counted(2));
}

#[test]
fn test_method_in_an_impl_is_not_counted() {
    let source = "struct S;\nimpl S {\n    #[cfg(test)]\n    fn helper() {}\n}\n";
    assert_eq!(measure_source(source).unwrap(), SourceLength::Counted(3));
}

#[test]
fn cfg_all_with_direct_test_is_not_counted() {
    let source = "#[cfg(all(test, unix))]\nfn only_in_unix_tests() {}\nfn always() {}\n";
    assert_eq!(measure_source(source).unwrap(), SourceLength::Counted(1));
}

#[test]
fn cfg_all_with_negated_test_is_counted() {
    let source = "#[cfg(all(not(test), unix))]\nfn outside_tests() {}\n";
    assert_eq!(measure_source(source).unwrap(), SourceLength::Counted(2));
}

#[test]
fn inner_cfg_test_makes_a_test_file_even_after_inner_docs() {
    let source = format!(
        "//! Shared test helpers.\n#![cfg(test)]\n{}",
        lines_of_code(500)
    );
    assert_eq!(measure_source(&source).unwrap(), SourceLength::TestFile);
}

#[test]
fn unparsable_source_is_an_error() {
    assert!(measure_source("fn broken( {").is_err());
}
