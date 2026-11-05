#![cfg(unix)]

use snet_wire::JsonValue;
use snet_wire::json::parse_strict;

use crate::errors::PseudonymError;
use crate::mapping::TaskMapping;
use crate::outbound::{Destination, OutboundJson};
use crate::test_support::{Site, cloud};

const PATTERN_SICIL: &str = "[[patterns]]\ncategory = \"SICIL\"\nregex = '\\d{6}(?: \\d{2})?'\n";
const PASS_EMAIL: &str = "[[passthrough]]\ncategory = \"EMAIL\"\nendpoint_name = \"main\"\nhost = \"api.example.com\"\nlegal_basis = \"Article 28 contract\"\n";

fn pseudonymised(
    site: &Site,
    names: &[&str],
    patterns: &str,
    passthrough: &str,
    texts: &[&str],
) -> Vec<String> {
    let policy = site.policy(names, patterns, passthrough);
    let pseudonymiser = policy.for_destination(cloud());
    let mut mapping = TaskMapping::new();
    texts
        .iter()
        .map(|text| {
            pseudonymiser
                .text(&mut mapping, text)
                .unwrap()
                .as_str()
                .to_string()
        })
        .collect()
}

#[test]
fn dictionary_terms_match_folded_on_word_boundaries() {
    let site = Site::new();
    let out = pseudonymised(
        &site,
        &["José", "ışık", "İstanbul", "Ali", "Ali Can"],
        "",
        "",
        &[
            "Jose\u{301} geldi",
            "IŞIK yandı",
            "ISTANBUL ofisi",
            "Alibaba ve Ali Cansu",
        ],
    );
    assert_eq!(out[0], "⟦PERSON_1⟧ geldi");
    assert_eq!(out[1], "⟦PERSON_2⟧ yandı");
    assert_eq!(out[2], "⟦PERSON_3⟧ ofisi");
    assert_eq!(out[3], "Alibaba ve ⟦PERSON_4⟧ Cansu");
}

#[test]
fn brackets_and_oversized_text_are_refused() {
    let site = Site::new();
    let policy = site.policy(&["Ali"], "", "");
    let pseudonymiser = policy.for_destination(cloud());
    let mut mapping = TaskMapping::new();
    for (text, position) in [("ab⟦c", 2), ("⟧", 0), ("x〚", 1), ("xy〛", 2)] {
        assert_eq!(
            pseudonymiser.text(&mut mapping, text).unwrap_err(),
            PseudonymError::Bracket { position }
        );
    }
    let large = "a".repeat(1024 * 1024 + 1);
    assert!(matches!(
        pseudonymiser.text(&mut mapping, &large),
        Err(PseudonymError::TextTooLarge { .. })
    ));
}

#[test]
fn overlaps_merge_and_rank() {
    let site = Site::new();
    let out = pseudonymised(
        &site,
        &["Ali Veli", "Veli Can"],
        "",
        "",
        &["Ali Veli Can geldi"],
    );
    assert_eq!(out[0], "⟦PERSON_1⟧ geldi");

    let pattern_eleven = "[[patterns]]\ncategory = \"SICIL\"\nregex = '\\d{11}'\n";
    let out = pseudonymised(&site, &["Ali"], pattern_eleven, "", &["no 12345678950"]);
    assert_eq!(out[0], "no ⟦SICIL_1⟧");

    let inside_phone = "[[patterns]]\ncategory = \"PART\"\nregex = '\\d{3} \\d{2}'\n";
    let out = pseudonymised(
        &site,
        &["Ali"],
        inside_phone,
        "",
        &["tel +90 532 123 45 67", "tel 0532 123 45 67"],
    );
    assert_eq!(out, vec!["tel ⟦PHONE_1⟧", "tel ⟦PHONE_1⟧"]);

    let out = pseudonymised(&site, &["Ali"], PATTERN_SICIL, "", &["sicil 123456 78b"]);
    assert_eq!(out[0], "sicil ⟦SICIL_1⟧ 78b");
}

#[test]
fn passthrough_keeps_its_category_only_for_its_destination() {
    let site = Site::new();
    let policy = site.policy(&["Ahmet"], "", PASS_EMAIL);
    let mut mapping = TaskMapping::new();
    let text = "mail ahmet.yilmaz@firma.com.tr";
    let passed = policy
        .for_destination(cloud())
        .text(&mut mapping, text)
        .unwrap();
    assert_eq!(passed.as_str(), "mail ⟦PERSON_1⟧.yilmaz@firma.com.tr");
    assert_eq!(passed.report().passed_through.get(&0), Some(&1));
    assert_eq!(passed.report().replaced.get("PERSON"), Some(&1));
    let other = Destination::new("main", "other.example.com");
    let replaced = policy
        .for_destination(other)
        .text(&mut mapping, text)
        .unwrap();
    assert_eq!(replaced.as_str(), "mail ⟦EMAIL_1⟧");
}

#[test]
fn equal_values_share_tokens_and_counters_are_per_category() {
    let site = Site::new();
    let out = pseudonymised(
        &site,
        &["Ahmet", "Veli"],
        "",
        "",
        &[
            "Ahmet, ali@x.com.tr, Veli",
            "AHMET +90 532 123 45 67",
            "ahmet 0532 123 45 67",
            "+44 (0) 20 7946 0958 and +44 20 7946 0958",
        ],
    );
    assert_eq!(out[0], "⟦PERSON_1⟧, ⟦EMAIL_1⟧, ⟦PERSON_2⟧");
    assert_eq!(out[1], "⟦PERSON_1⟧ ⟦PHONE_1⟧");
    assert_eq!(out[2], "⟦PERSON_1⟧ ⟦PHONE_1⟧");
    assert_eq!(out[3], "⟦PHONE_2⟧ and ⟦PHONE_2⟧");
}

#[test]
fn the_mapping_limits_hold_and_failures_change_nothing() {
    let site = Site::new();
    let policy = site.policy(&["Ali"], "", "");
    let pseudonymiser = policy.for_destination(cloud());
    let mut mapping = TaskMapping::new();
    pseudonymiser.text(&mut mapping, "Ali").unwrap();
    let before = mapping.to_bytes();

    let many: String = (0..10_000)
        .map(|index| format!("u{index}@x.com.tr "))
        .collect();
    assert_eq!(
        pseudonymiser.text(&mut mapping, &many).unwrap_err(),
        PseudonymError::MappingFull {
            limit: "10 000 entries"
        }
    );
    assert_eq!(*mapping.to_bytes(), *before);

    let json = parse_strict(br#"{"a":"veli@x.com.tr","b":12345678950}"#).unwrap();
    assert!(matches!(
        pseudonymiser.json(&mut mapping, &json),
        Err(PseudonymError::NumberMatched { .. })
    ));
    assert_eq!(*mapping.to_bytes(), *before);
}

#[test]
fn json_values_are_pseudonymised_value_by_value() {
    let site = Site::new();
    let policy = site.policy(
        &["Ali", "Ayşe"],
        "[[patterns]]\ncategory = \"SICIL\"\nregex = '\\d{7}'\n",
        "",
    );
    let pseudonymiser = policy.for_destination(cloud());
    let mut mapping = TaskMapping::new();
    let json =
        parse_strict(br#"{"note":"Rapor:\nAli geldi","who":"Ay\u015fe","count":3}"#).unwrap();
    let out = pseudonymiser.json(&mut mapping, &json).unwrap();
    assert_eq!(
        out.as_str(),
        r#"{"count":3,"note":"Rapor:\n⟦PERSON_1⟧ geldi","who":"⟦PERSON_2⟧"}"#
    );

    let refused_number = |bytes: &[u8]| {
        let mut fresh = TaskMapping::new();
        pseudonymiser.json(&mut fresh, &parse_strict(bytes).unwrap())
    };
    assert_eq!(
        refused_number(br#"{"id":12345678950}"#).unwrap_err(),
        PseudonymError::NumberMatched {
            path: "/0".to_string(),
            category: "TR_IDENTITY".to_string()
        }
    );
    assert!(refused_number(br#"{"id":12345678950.0}"#).is_err());
    assert!(refused_number(br#"{"id":12345678901.0}"#).is_ok());
    assert!(matches!(
        refused_number(br#"{"rows":[1,{"employee":1234567}]}"#),
        Err(PseudonymError::NumberMatched { path, category }) if path == "/0/1/0" && category == "SICIL"
    ));
    assert_eq!(
        refused_number(br#"{"Ali":1,"ALI":2}"#).unwrap_err(),
        PseudonymError::KeysCollide {
            path: "/".to_string()
        }
    );

    let mut deep = JsonValue::Null;
    for _ in 0..65 {
        deep = JsonValue::Array(vec![deep]);
    }
    assert_eq!(
        pseudonymiser.json(&mut mapping, &deep).unwrap_err(),
        PseudonymError::JsonTooDeep
    );
    let large = JsonValue::String("a".repeat(1024 * 1024));
    assert!(matches!(
        pseudonymiser.json(&mut mapping, &large),
        Err(PseudonymError::JsonTooLarge { .. })
    ));
}

#[test]
fn typed_json_and_joins_check_destination_and_policy() {
    let site = Site::new();
    let policy = site.policy(&["Ali"], "", "");
    let other_policy = site.policy(&["Veli"], "", "");
    let pseudonymiser = policy.for_destination(cloud());
    let mut mapping = TaskMapping::new();
    let answer = pseudonymiser.text(&mut mapping, "Ali geldi").unwrap();
    let typed = pseudonymiser
        .typed(&OutboundJson::Object(vec![
            ("round", OutboundJson::Integer(2)),
            ("final", OutboundJson::Bool(false)),
            ("none", OutboundJson::Null),
            ("step", OutboundJson::Static("ask")),
            (
                "citations",
                OutboundJson::Array(vec![OutboundJson::Numbered("C", 3)]),
            ),
            ("answer", OutboundJson::Text(answer.clone())),
        ]))
        .unwrap();
    assert_eq!(
        typed.as_str(),
        r#"{"round":2,"final":false,"none":null,"step":"ask","citations":["C3"],"answer":"⟦PERSON_1⟧ geldi"}"#
    );
    assert_eq!(typed.report().replaced.get("PERSON"), Some(&1));
    assert_eq!(
        pseudonymiser
            .typed(&OutboundJson::Integer(1_000_000))
            .unwrap_err(),
        PseudonymError::IntegerOutOfRange { value: 1_000_000 }
    );

    let elsewhere = policy.for_destination(Destination::new("other", "api.example.com"));
    let foreign = elsewhere.text(&mut mapping, "Ali").unwrap();
    assert_eq!(
        pseudonymiser
            .typed(&OutboundJson::Text(foreign.clone()))
            .unwrap_err(),
        PseudonymError::DestinationMismatch
    );
    assert_eq!(
        answer.clone().join(&foreign).unwrap_err(),
        PseudonymError::DestinationMismatch
    );
    let other = other_policy.for_destination(cloud()).template("fixed");
    assert_eq!(
        answer.clone().join(&other).unwrap_err(),
        PseudonymError::PolicyMismatch
    );
    let joined = pseudonymiser.template("Note: ").join(&answer).unwrap();
    assert_eq!(joined.as_str(), "Note: ⟦PERSON_1⟧ geldi");
    assert!(!format!("{joined:?}").contains("geldi"));
}

#[test]
fn dictionaries_share_categories_and_the_first_wins_a_tie() {
    let site = Site::new();
    let staff = site.dictionary("staff.txt", "Jose\u{301}\nAli\n".as_bytes());
    let people = site.dictionary("people.txt", b"Ali\nVeli\n");
    let visitors = site.dictionary("visitors.txt", b"Can\n");
    let text = format!(
        "patterns = []\npassthrough = []\n{}\n[[dictionaries]]\ncategory = \"STAFF\"\npath = \"{}\"\n[[dictionaries]]\ncategory = \"PERSON\"\npath = \"{}\"\n[[dictionaries]]\ncategory = \"PERSON\"\npath = \"{}\"\n",
        crate::test_support::ALL_BUILTINS,
        staff.display(),
        people.display(),
        visitors.display()
    );
    let policy = crate::policy::Policy::parse(&text).unwrap();
    let mut mapping = TaskMapping::new();
    let out = policy
        .for_destination(cloud())
        .text(&mut mapping, "José, Ali, Veli and Can")
        .unwrap();
    assert_eq!(
        out.as_str(),
        "⟦STAFF_1⟧, ⟦STAFF_2⟧, ⟦PERSON_1⟧ and ⟦PERSON_2⟧"
    );
}

#[test]
fn the_mapping_byte_limit_holds() {
    let site = Site::new();
    let policy = site.policy(
        &["Ali"],
        "[[patterns]]\ncategory = \"LONG\"\nregex = 'X[a-z]{255}'\n",
        "",
    );
    let pseudonymiser = policy.for_destination(cloud());
    let mut mapping = TaskMapping::new();
    let value = |index: usize| {
        let mut letters = String::from("X");
        let mut rest = index;
        for _ in 0..255 {
            letters.push(char::from(b'a' + (rest % 26) as u8));
            rest /= 26;
        }
        letters
    };
    let mut refused = None;
    for call in 0..4 {
        let text: String = (call * 2500..(call + 1) * 2500)
            .map(|index| format!("{} ", value(index)))
            .collect();
        if let Err(error) = pseudonymiser.text(&mut mapping, &text) {
            refused = Some(error);
            break;
        }
    }
    assert_eq!(
        refused,
        Some(PseudonymError::MappingFull {
            limit: "4 MiB of values"
        })
    );
    assert!(mapping.len() < 10_000);
}

/// Run by hand in a release build: the time to pseudonymise 1 MiB of
/// factory-like text with numbers, readings and a value every line.
#[test]
#[ignore]
fn a_full_size_text_takes_bounded_time() {
    let site = Site::new();
    let policy = site.policy(&["Ali Veli", "Zeynep"], PATTERN_SICIL, "");
    let pseudonymiser = policy.for_destination(cloud());
    let line = "2026-11-05 07:31:12 line 3 valve 12.5 bar 1730800000000 Zeynep +90 532 123 45 67 part 4711-22 ok\n";
    let text = line.repeat(1024 * 1024 / line.len());
    let mut mapping = TaskMapping::new();
    let started = std::time::Instant::now();
    let out = pseudonymiser.text(&mut mapping, &text).unwrap();
    println!(
        "{} bytes in {:?}, {} replaced",
        text.len(),
        started.elapsed(),
        out.report().replaced.values().sum::<u64>()
    );
}
