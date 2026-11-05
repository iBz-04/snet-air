#![cfg(unix)]

use crate::errors::PolicyError;
use crate::policy::Policy;
use crate::test_support::{ALL_BUILTINS, Fixture, policy_text};

const MARKER_TERM: &str = "Zeynepmarker";

fn refused(text: &str) -> PolicyError {
    Policy::parse(text).unwrap_err()
}

fn assert_holds_no_marker(error: &PolicyError) {
    assert!(!error.to_string().contains(MARKER_TERM), "{error}");
    assert!(!format!("{error:?}").contains(MARKER_TERM), "{error:?}");
}

#[test]
fn a_full_policy_and_empty_arrays_load() {
    let fixture = Fixture::new();
    let policy = fixture.policy(&["Ahmet Yılmaz", "Ayşe"], "", "");
    assert_eq!(policy.dictionary_categories, vec!["PERSON".to_string()]);
    assert_eq!(policy.enabled.len(), 7);
    let empty = format!("dictionaries = []\npatterns = []\npassthrough = []\n{ALL_BUILTINS}");
    assert!(Policy::parse(&empty).unwrap().automaton.is_none());
    let tables = format!(
        "dictionaries = []\n{ALL_BUILTINS}\n[[patterns]]\ncategory = \"SICIL\"\nregex = 'S-\\d{{6}}'\n[[patterns]]\ncategory = \"SICIL\"\nregex = 'P-\\d{{4}}'\n[[passthrough]]\ncategory = \"EMAIL\"\nendpoint_name = \"main\"\nhost = \"api.example.com\"\nlegal_basis = \"Contract with the provider, Article 28\"\n"
    );
    let loaded = Policy::parse(&tables).unwrap();
    assert_eq!(loaded.patterns.len(), 2);
    assert_eq!(loaded.passthrough_rules().len(), 1);
}

#[test]
fn the_builtin_policy_enables_every_detector_and_nothing_else() {
    let policy = Policy::builtin().unwrap();
    assert_eq!(policy.enabled.len(), 7);
    assert!(policy.automaton.is_none());
    assert!(policy.patterns.is_empty());
    assert!(policy.passthrough_rules().is_empty());
}

#[test]
fn missing_unknown_and_mistyped_fields_are_named() {
    let base = format!("dictionaries = []\npatterns = []\npassthrough = []\n{ALL_BUILTINS}");
    assert_eq!(
        refused(&base.replace("patterns = []\n", "")),
        PolicyError::Missing {
            field: "patterns".to_string()
        }
    );
    assert_eq!(
        refused(&format!("extra = 1\n{base}")),
        PolicyError::Unknown {
            field: "extra".to_string()
        }
    );
    assert_eq!(
        refused(&base.replace("MAC_ADDRESS = true\n", "")),
        PolicyError::Missing {
            field: "builtin.MAC_ADDRESS".to_string()
        }
    );
    assert!(matches!(
        refused(&base.replace("EMAIL = true", &format!("EMAIL = \"{MARKER_TERM}\""))),
        PolicyError::Invalid { field, .. } if field == "builtin.EMAIL"
    ));
    assert!(matches!(
        refused("not toml = ="),
        PolicyError::Syntax { line: 1, .. }
    ));
    let dictionary_without_path = format!(
        "patterns = []\npassthrough = []\n{ALL_BUILTINS}\n[[dictionaries]]\ncategory = \"PERSON\"\n"
    );
    assert_eq!(
        refused(&dictionary_without_path),
        PolicyError::Missing {
            field: "dictionaries[0].path".to_string()
        }
    );
}

#[test]
fn category_names_are_checked() {
    let with_category = |name: &str| {
        format!(
            "dictionaries = []\npassthrough = []\n{ALL_BUILTINS}\n[[patterns]]\ncategory = \"{name}\"\nregex = 'S-\\d{{6}}'\n"
        )
    };
    for name in [
        "person",
        "1ID",
        "",
        "A23456789012345678901234567890123",
        "EMAIL",
        "A-B",
    ] {
        assert!(
            matches!(refused(&with_category(name)), PolicyError::Invalid { field, .. } if field == "patterns[0].category"),
            "{name}"
        );
    }
    assert!(Policy::parse(&with_category("A2345678901234567890123456789012")).is_ok());
}

#[test]
fn dictionary_files_are_checked() {
    let fixture = Fixture::new();
    let load = |bytes: &[u8]| {
        let path = fixture.dictionary("terms.txt", bytes);
        Policy::parse(&policy_text(&path, "", ""))
    };
    let line_error = |bytes: &[u8]| match load(bytes).unwrap_err() {
        PolicyError::DictionaryLine { line, reason, .. } => (line, reason),
        other => panic!("{other}"),
    };
    assert_eq!(line_error(b"\xEF\xBB\xBFAli\n").0, 1);
    assert_eq!(line_error(b"Ali\n\xFF\xFE\n").0, 2);
    assert_eq!(line_error(b"Ali\n\nVeli\n").0, 2);
    assert_eq!(line_error(b"Ali\nA\n").0, 2);
    assert_eq!(line_error(format!("{}\n", "a".repeat(201)).as_bytes()).0, 1);
    assert_eq!(line_error(format!("{MARKER_TERM}.\n").as_bytes()).0, 1);
    for error in [
        load(format!("{MARKER_TERM}.\n").as_bytes()).unwrap_err(),
        load(format!("x\n{MARKER_TERM}\n").as_bytes()).unwrap_err(),
    ] {
        assert_holds_no_marker(&error);
    }
    assert!(load(format!("{}\r\nVeli\r\n", "a".repeat(200)).as_bytes()).is_ok());
    let duplicates = load(b"Ali\nALI\nali\n").unwrap();
    assert_eq!(duplicates.term_dictionary.len(), 1);

    let relative = policy_text(std::path::Path::new("terms.txt"), "", "");
    assert!(
        matches!(refused(&relative), PolicyError::Invalid { field, .. } if field == "dictionaries[0].path")
    );
}

#[test]
fn site_patterns_are_checked_without_their_text_in_errors() {
    let with_pattern = |regex: &str| {
        format!(
            "dictionaries = []\npassthrough = []\n{ALL_BUILTINS}\n[[patterns]]\ncategory = \"SICIL\"\nregex = '{regex}'\n"
        )
    };
    for regex in [
        format!("{MARKER_TERM}("),
        format!("({MARKER_TERM})?"),
        format!("{MARKER_TERM}\\d+"),
        format!("{MARKER_TERM}{}", "a".repeat(250)),
        "a".repeat(1025),
        "\\p{L}{200}\\p{L}{56}x".to_string(),
    ] {
        let error = refused(&with_pattern(&regex));
        assert!(
            matches!(error, PolicyError::Pattern { position: 0, .. }),
            "{error}"
        );
        assert_holds_no_marker(&error);
    }
}

#[test]
fn limits_and_passthrough_rules_are_checked() {
    let fixture = Fixture::new();
    let many = (0..17)
        .map(|index| {
            let path = fixture.dictionary(&format!("d{index}.txt"), b"Ali\n");
            format!(
                "[[dictionaries]]\ncategory = \"PERSON\"\npath = \"{}\"\n",
                path.display()
            )
        })
        .collect::<String>();
    assert_eq!(
        refused(&format!(
            "patterns = []\npassthrough = []\n{ALL_BUILTINS}\n{many}"
        )),
        PolicyError::Limit {
            limit: "16 dictionaries"
        }
    );
    let rule = |category: &str, basis: &str| {
        format!(
            "dictionaries = []\npatterns = []\n{ALL_BUILTINS}\n[[passthrough]]\ncategory = \"{category}\"\nendpoint_name = \"main\"\nhost = \"api.example.com\"\nlegal_basis = \"{basis}\"\n"
        )
    };
    assert!(
        matches!(refused(&rule("PERSON", "Art. 28")), PolicyError::Invalid { field, .. } if field == "passthrough[0].category")
    );
    assert!(
        matches!(refused(&rule("EMAIL", "")), PolicyError::Invalid { field, .. } if field == "passthrough[0].legal_basis")
    );
    assert!(matches!(
        refused(&rule("EMAIL", &"b".repeat(201))),
        PolicyError::Invalid { .. }
    ));
}

#[test]
fn the_digest_covers_every_file_and_where_each_line_belongs() {
    let fixture = Fixture::new();
    let first = fixture.dictionary("first.txt", b"Ali\nVeli\n");
    let second = fixture.dictionary("second.txt", b"Can\n");
    let text = format!(
        "patterns = []\npassthrough = []\n{ALL_BUILTINS}\n[[dictionaries]]\ncategory = \"A\"\npath = \"{}\"\n[[dictionaries]]\ncategory = \"B\"\npath = \"{}\"\n",
        first.display(),
        second.display()
    );
    let before = Policy::parse(&text).unwrap().digest();
    fixture.dictionary("first.txt", b"Ali\n");
    fixture.dictionary("second.txt", b"Veli\nCan\n");
    let moved = Policy::parse(&text).unwrap().digest();
    assert_ne!(before, moved);
    fixture.dictionary("second.txt", b"Veli\nCam\n");
    assert_ne!(moved, Policy::parse(&text).unwrap().digest());
}

/// The policy size bound, run by hand under `/usr/bin/time -l` for the peak resident
/// memory: 16 dictionaries holding 200 000 distinct terms in all, of 30 to
/// 53 ASCII letters each (about 8.3 million folded bytes, under the 8 MiB
/// cap of 8 388 608).
#[test]
#[ignore]
fn the_automaton_at_the_limits_stays_within_its_memory_bound() {
    let fixture = Fixture::new();
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut tables = String::new();
    let mut total_bytes = 0usize;
    let per_dictionary = 200_000 / 16;
    for dictionary in 0..16 {
        let mut text = String::new();
        for _ in 0..per_dictionary {
            let length = 30 + (next() % 24) as usize;
            total_bytes += length;
            for _ in 0..length {
                text.push(char::from(b'a' + (next() % 26) as u8));
            }
            text.push('\n');
        }
        let path = fixture.dictionary(&format!("d{dictionary}.txt"), text.as_bytes());
        tables.push_str(&format!(
            "[[dictionaries]]\ncategory = \"PERSON\"\npath = \"{}\"\n",
            path.display()
        ));
    }
    let started = std::time::Instant::now();
    let policy = Policy::parse(&format!(
        "patterns = []\npassthrough = []\n{ALL_BUILTINS}\n{tables}"
    ))
    .unwrap();
    let usage = policy.automaton.as_ref().unwrap().memory_usage();
    println!(
        "terms {} folded bytes {total_bytes} automaton {} MiB build {:?}",
        policy.term_dictionary.len(),
        usage / (1024 * 1024),
        started.elapsed()
    );
    assert!(usage <= 256 * 1024 * 1024);
}

#[test]
fn a_dictionary_over_16_mib_is_refused() {
    let fixture = Fixture::new();
    let path = fixture.dictionary("huge.txt", &vec![b'a'; 16 * 1024 * 1024 + 1]);
    match refused(&policy_text(&path, "", "")) {
        PolicyError::Dictionary { reason, .. } => assert!(reason.contains("16 MiB"), "{reason}"),
        other => panic!("{other}"),
    }
}
