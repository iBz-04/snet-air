#![cfg(unix)]

use snet_wire::json::parse_strict;

use crate::errors::PseudonymError;
use crate::mapping::TaskMapping;
use crate::policy::Policy;
use crate::test_support::{Site, cloud};

const SICIL: &str = "[[patterns]]\ncategory = \"SICIL\"\nregex = '\\d{3}'\n";

fn setup(site: &Site) -> Policy {
    site.policy(&["Ali", "Veli"], SICIL, "")
}

#[test]
fn a_round_trip_restores_the_first_surface_forms() {
    let site = Site::new();
    let policy = setup(&site);
    let pseudonymiser = policy.for_destination(cloud());
    let mut mapping = TaskMapping::new();
    let sent = pseudonymiser
        .text(
            &mut mapping,
            "Ali called +90 532 123 45 67; ALI wrote ali@x.com.tr.",
        )
        .unwrap();
    assert_eq!(
        sent.as_str(),
        "⟦PERSON_1⟧ called ⟦PHONE_1⟧; ⟦PERSON_1⟧ wrote ⟦EMAIL_1⟧."
    );
    let restored = mapping.restore(sent.as_str()).unwrap();
    assert_eq!(
        restored.as_str(),
        "Ali called +90 532 123 45 67; Ali wrote ali@x.com.tr."
    );
    let texts: Vec<&str> = restored
        .ranges()
        .iter()
        .map(|range| &restored.as_str()[range.clone()])
        .collect();
    assert_eq!(
        texts,
        vec!["Ali", "+90 532 123 45 67", "Ali", "ali@x.com.tr"]
    );
    assert!(!format!("{restored:?}").contains("Ali"));
}

#[test]
fn separators_keep_restored_values_detectable_in_the_next_round() {
    let site = Site::new();
    let policy = setup(&site);
    let pseudonymiser = policy.for_destination(cloud());
    let mut mapping = TaskMapping::new();
    pseudonymiser
        .text(
            &mut mapping,
            "Ali Veli 12345678950 +90 532 123 45 67 ali@x.com.tr 2001:db8::1 999",
        )
        .unwrap();
    let cases = [
        ("⟦PERSON_1⟧nin", "Ali'nin"),
        ("⟦PERSON_1⟧⟦PERSON_2⟧", "Ali'Veli"),
        ("3.⟦TR_IDENTITY_1⟧", "3. 12345678950"),
        ("1,⟦PHONE_1⟧", "1, +90 532 123 45 67"),
        (".⟦EMAIL_1⟧", ".ali@x.com.tr"),
        ("1.⟦EMAIL_1⟧", "1. ali@x.com.tr"),
        ("⟦EMAIL_1⟧-x", "ali@x.com.tr -x"),
        ("⟦IP_ADDRESS_1⟧:", "2001:db8::1 :"),
        ("12-⟦SICIL_1⟧", "12- 999"),
        ("(⟦PERSON_1⟧).", "(Ali)."),
        ("⟦PERSON_1⟧.5", "Ali .5"),
    ];
    for (cloud_text, expected) in cases {
        let restored = mapping.restore(cloud_text).unwrap();
        assert_eq!(restored.as_str(), expected, "{cloud_text}");
        let again = pseudonymiser.text(&mut mapping, restored.as_str()).unwrap();
        for range in restored.ranges() {
            let value = &restored.as_str()[range.clone()];
            assert!(
                !again.as_str().contains(value),
                "{cloud_text}: {}",
                again.as_str()
            );
        }
    }
}

#[test]
fn foreign_malformed_and_stray_tokens_are_refused() {
    let site = Site::new();
    let policy = setup(&site);
    let mut mapping = TaskMapping::new();
    policy
        .for_destination(cloud())
        .text(&mut mapping, "Ali")
        .unwrap();
    let cases = [
        ("x ⟦PERSON_2⟧", PseudonymError::TokenUnknown { position: 2 }),
        ("⟦PERSON 2⟧", PseudonymError::TokenMalformed { position: 0 }),
        (
            "⟦PERSON_01⟧",
            PseudonymError::TokenMalformed { position: 0 },
        ),
        ("⟦PERSON_1", PseudonymError::TokenMalformed { position: 0 }),
        ("ok ⟧", PseudonymError::StrayBracket { position: 3 }),
        ("〚PERSON_1〛", PseudonymError::StrayBracket { position: 0 }),
        (
            "⟦PERSON_1〛",
            PseudonymError::TokenMalformed { position: 0 },
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(mapping.restore(text).unwrap_err(), expected, "{text}");
    }
}

#[test]
fn restored_json_is_serialised_and_colliding_keys_refused() {
    let site = Site::new();
    let policy = setup(&site);
    let mut mapping = TaskMapping::new();
    policy
        .for_destination(cloud())
        .text(&mut mapping, "Ali")
        .unwrap();
    let reply = parse_strict("{\"text\":\"⟦PERSON_1⟧ came\",\"n\":2}".as_bytes()).unwrap();
    assert_eq!(
        mapping.restore_json(&reply).unwrap().as_str(),
        r#"{"n":2,"text":"Ali came"}"#
    );
    let colliding = parse_strict("{\"⟦PERSON_1⟧\":1,\"Ali\":2}".as_bytes()).unwrap();
    assert_eq!(
        mapping.restore_json(&colliding).unwrap_err(),
        PseudonymError::KeysCollide {
            path: "/".to_string()
        }
    );
}

#[test]
fn the_spelled_value_check_finds_reshaped_values() {
    let site = Site::new();
    let policy = setup(&site);
    let mut mapping = TaskMapping::new();
    policy
        .for_destination(cloud())
        .text(&mut mapping, "Ali +90 532 123 45 67 ali.veli@x.com.tr")
        .unwrap();
    assert!(mapping.holds_spelled_value("call 5 3 2 - 1 2 3 - 4 5 - 6 7"));
    assert!(mapping.holds_spelled_value("national 0532 123 4567"));
    assert!(mapping.holds_spelled_value("ALI . VELI (x) com tr"));
    assert!(!mapping.holds_spelled_value("Ali was there"));
}
