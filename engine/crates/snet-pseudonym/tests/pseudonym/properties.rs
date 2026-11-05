#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use proptest::prelude::*;
use snet_pseudonym::{Destination, JsonValue, Policy, TaskMapping};

const VALUES: &[&str] = &[
    "Zeynep Kaya",
    "ayse.kaya@plant.com.tr",
    "+90 532 765 43 21",
    "TR33 0006 1005 1978 6457 8413 26",
    "4111 1111 1111 1111",
    "10000000146",
    "192.168.10.7",
    "fe80::12",
    "00:1A:2B:3C:4D:5E",
    "S-123456",
];
const WORDS: &[&str] = &[
    "line", "shift", "valve", "pressure", "stop", "ok", "night", ",", ".", ";", "(", ")", "-", "!",
];

struct Site {
    _directory: tempfile::TempDir,
    policy: Policy,
}

fn site() -> Site {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), PermissionsExt::from_mode(0o700)).unwrap();
    let dictionary = directory.path().join("people.txt");
    std::fs::write(&dictionary, "Zeynep Kaya\n").unwrap();
    std::fs::set_permissions(&dictionary, PermissionsExt::from_mode(0o600)).unwrap();
    let policy_path = directory.path().join("policy.toml");
    std::fs::write(
        &policy_path,
        format!(
            "passthrough = []\n[builtin]\nEMAIL = true\nPHONE = true\nIBAN = true\nPAYMENT_CARD = true\nTR_IDENTITY = true\nIP_ADDRESS = true\nMAC_ADDRESS = true\n\n[[dictionaries]]\ncategory = \"PERSON\"\npath = \"{}\"\n\n[[patterns]]\ncategory = \"SICIL\"\nregex = 'S-\\d{{6}}'\n",
            dictionary.display()
        ),
    )
    .unwrap();
    let policy = Policy::load(&policy_path).unwrap();
    Site {
        _directory: directory,
        policy,
    }
}

fn cloud() -> Destination {
    Destination::new("main", "api.example.com")
}

fn planted_text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        3 => proptest::sample::select(WORDS).prop_map(str::to_string),
        1 => proptest::sample::select(VALUES).prop_map(str::to_string),
    ];
    proptest::collection::vec(piece, 1..40).prop_map(|pieces| pieces.join(" "))
}

/// Neighbours a restored value may meet: every ASCII punctuation mark and
/// one character of each Unicode general category.
fn neighbours() -> Vec<char> {
    let mut characters: Vec<char> = (0x21u8..0x7F)
        .map(char::from)
        .filter(|character| character.is_ascii_punctuation())
        .collect();
    characters.extend([
        'a', 'Z', 'ǅ', 'ʰ', 'ª', '\u{301}', '\u{903}', '\u{20DD}', '7', 'Ⅻ', '½', '_', '‐', '「',
        '」', '«', '»', '¡', '+', '$', '^', '©', ' ', '\u{2028}', '\u{2029}', 'ş', 'İ', 'ı', '⁴',
    ]);
    characters
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn planted_values_never_survive_and_restore_round_trips(text in planted_text()) {
        let site = site();
        let pseudonymiser = site.policy.for_destination(cloud());
        let mut mapping = TaskMapping::new();
        let sent = pseudonymiser.text(&mut mapping, &text).unwrap();
        for value in VALUES {
            prop_assert!(!sent.as_str().contains(value), "{} in {}", value, sent.as_str());
        }
        let restored = mapping.restore(sent.as_str()).unwrap();
        prop_assert_eq!(restored.as_str(), text.as_str());
        let again = pseudonymiser.text(&mut mapping, restored.as_str()).unwrap();
        prop_assert_eq!(again.as_str(), sent.as_str());
    }

    #[test]
    fn pseudonymise_and_restore_never_panic(text in ".{0,200}", bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
        let site = site();
        let pseudonymiser = site.policy.for_destination(cloud());
        let mut mapping = TaskMapping::new();
        let _ = pseudonymiser.text(&mut mapping, &text);
        let _ = mapping.restore(&text);
        let _ = TaskMapping::from_bytes(&bytes);
        let _ = mapping.holds_spelled_value(&text);
    }

    #[test]
    fn json_walks_never_panic(value in json_value()) {
        let site = site();
        let pseudonymiser = site.policy.for_destination(cloud());
        let mut mapping = TaskMapping::new();
        let _ = pseudonymiser.json(&mut mapping, &value);
        let _ = mapping.restore_json(&value);
    }
}

fn json_value() -> impl Strategy<Value = JsonValue> {
    let leaf = prop_oneof![
        Just(JsonValue::Null),
        any::<bool>().prop_map(JsonValue::Bool),
        any::<i64>().prop_map(|number| JsonValue::Number(number.into())),
        ".{0,40}".prop_map(JsonValue::String),
        proptest::sample::select(VALUES).prop_map(|value| JsonValue::String(value.to_string())),
    ];
    leaf.prop_recursive(6, 64, 6, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..6).prop_map(JsonValue::Array),
            proptest::collection::btree_map(".{0,12}", inner, 0..6).prop_map(JsonValue::Object),
        ]
    })
}

#[test]
fn restored_values_are_caught_again_next_to_any_neighbour() {
    let site = site();
    let pseudonymiser = site.policy.for_destination(cloud());
    let mut mapping = TaskMapping::new();
    let sent = pseudonymiser
        .text(&mut mapping, &VALUES.join(" ; "))
        .unwrap();
    let tokens: Vec<&str> = sent.as_str().split(" ; ").collect();
    assert_eq!(tokens.len(), VALUES.len(), "{}", sent.as_str());
    for token in &tokens {
        for neighbour in neighbours() {
            for cloud_text in [
                format!("{neighbour}{token}"),
                format!("{token}{neighbour}"),
                format!("{neighbour}{token}{neighbour}"),
                format!("{neighbour}{neighbour}{token}{neighbour}{neighbour}"),
            ] {
                let restored = mapping.restore(&cloud_text).unwrap();
                let again = pseudonymiser.text(&mut mapping, restored.as_str()).unwrap();
                for range in restored.ranges() {
                    let value = &restored.as_str()[range.clone()];
                    assert!(
                        !again.as_str().contains(value),
                        "{cloud_text:?} restored as {:?} left {value:?} in {:?}",
                        restored.as_str(),
                        again.as_str()
                    );
                }
            }
        }
    }
}

#[test]
fn errors_and_debug_output_hold_no_values() {
    const MARKER: &str = "Zeynep Kaya";
    let site = site();
    let pseudonymiser = site.policy.for_destination(cloud());
    let mut mapping = TaskMapping::new();
    let sent = pseudonymiser
        .text(&mut mapping, &format!("{MARKER} geldi"))
        .unwrap();
    let restored = mapping.restore(sent.as_str()).unwrap();
    let reply = snet_pseudonym::JsonValue::String(sent.as_str().to_string());
    let restored_json = mapping.restore_json(&reply).unwrap();
    let errors = [
        pseudonymiser
            .text(&mut mapping, &format!("{MARKER} ⟦"))
            .unwrap_err(),
        mapping
            .restore(&format!("{MARKER} ⟦PERSON_9⟧"))
            .unwrap_err(),
        mapping.restore(&format!("{MARKER} ⟧")).unwrap_err(),
    ];
    let printed = [
        format!("{sent:?}"),
        format!("{restored:?}"),
        format!("{restored_json:?}"),
        format!("{mapping:?}"),
        format!("{:?}", site.policy),
    ];
    for text in printed.iter().chain(
        errors
            .iter()
            .map(|error| format!("{error} {error:?}"))
            .collect::<Vec<_>>()
            .iter(),
    ) {
        assert!(!text.contains("Zeynep"), "{text}");
    }
}
