use crate::support::{PLANTED_WORD, digest_hex, terms_listing};
use snet_rules::errors::RulesError;
use snet_rules::forbidden_terms::ForbiddenTerms;

#[test]
fn compiled_list_parses_and_is_not_empty() {
    let terms = ForbiddenTerms::compiled().unwrap();
    assert!(!terms.is_empty());
}

#[test]
fn empty_list_is_refused() {
    assert!(matches!(
        ForbiddenTerms::parse(""),
        Err(RulesError::ForbiddenTermsEmpty)
    ));
    assert!(matches!(
        ForbiddenTerms::parse("\n"),
        Err(RulesError::ForbiddenTermsEmpty)
    ));
}

#[test]
fn malformed_lines_are_refused_with_their_line_number() {
    let valid = digest_hex(PLANTED_WORD).unwrap();
    let uppercase = valid.to_uppercase();
    let short = &valid[..63];

    for (list, bad_line) in [
        (format!("{valid}\n{uppercase}\n"), 2),
        (format!("{short}\n"), 1),
        (format!("{valid}\n\n"), 2),
        (format!("{valid}\n{valid}x\n"), 2),
    ] {
        match ForbiddenTerms::parse(&list) {
            Err(RulesError::ForbiddenTermMalformed { line }) => assert_eq!(line, bad_line),
            other => panic!("expected a malformed line {bad_line}, got {other:?}"),
        }
    }
}

#[test]
fn one_trailing_newline_is_allowed_and_none_is_too() {
    let valid = digest_hex(PLANTED_WORD).unwrap();
    assert_eq!(
        ForbiddenTerms::parse(&format!("{valid}\n")).unwrap().len(),
        1
    );
    assert_eq!(ForbiddenTerms::parse(&valid).unwrap().len(), 1);
}

#[test]
fn listed_words_are_found_by_line_in_any_case() {
    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    let text = "clean line\nthe PlantedName client\nalso_plantedname_here\nplantednames differ\n";
    let lines: Vec<usize> = terms
        .lines_with_listed_words(text.as_bytes())
        .into_iter()
        .collect();
    assert_eq!(lines, vec![2, 3]);
}

#[test]
fn display_path_masks_only_listed_components() {
    let terms = terms_listing(&[PLANTED_WORD]).unwrap();
    assert_eq!(
        terms.display_path("docs/plantedname_notes/readme.md"),
        "docs/<listed:2>/readme.md"
    );
    assert_eq!(terms.display_path("docs/readme.md"), "docs/readme.md");
}
