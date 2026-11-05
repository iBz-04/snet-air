#![cfg(unix)]

use crate::errors::PseudonymError;
use crate::mapping::TaskMapping;
use crate::test_support::{Site, cloud};

fn filled() -> TaskMapping {
    let site = Site::new();
    let policy = site.policy(&["Ali"], "", "");
    let mut mapping = TaskMapping::new();
    policy
        .for_destination(cloud())
        .text(&mut mapping, "Ali a@x.com.tr b@x.com.tr +90 532 123 45 67")
        .unwrap();
    mapping
}

fn word(value: u32) -> [u8; 4] {
    value.to_be_bytes()
}

fn refused(bytes: &[u8]) -> &'static str {
    match TaskMapping::from_bytes(bytes).unwrap_err() {
        PseudonymError::MappingBytes { reason } => reason,
        other => panic!("{other}"),
    }
}

/// One category `PERSON` with the given counter and entry count, then the
/// given entries as (category, number, canonical, surface).
fn encoded(counter: u32, declared: u32, entries: &[(u32, u32, &str, &str)]) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend(word(1));
    bytes.extend(word(6));
    bytes.extend(b"PERSON");
    bytes.extend(word(counter));
    bytes.extend(word(declared));
    for (category, number, canonical, surface) in entries {
        bytes.extend(word(*category));
        bytes.extend(word(*number));
        bytes.extend(word(canonical.len() as u32));
        bytes.extend(canonical.as_bytes());
        bytes.extend(word(surface.len() as u32));
        bytes.extend(surface.as_bytes());
    }
    bytes
}

#[test]
fn a_mapping_serialises_and_reads_back_byte_for_byte() {
    let mapping = filled();
    let bytes = mapping.to_bytes();
    assert_eq!(bytes.len(), bytes.capacity());
    let read = TaskMapping::from_bytes(&bytes).unwrap();
    assert_eq!(*read.to_bytes(), *bytes);
    assert_eq!(read.restore("⟦EMAIL_2⟧").unwrap().as_str(), "b@x.com.tr");
    assert!(!format!("{read:?}").contains("Ali"));
    assert!(TaskMapping::from_bytes(&encoded(1, 1, &[(0, 1, "ali", "Ali")])).is_ok());
}

#[test]
fn every_fault_in_the_bytes_is_refused() {
    let good = encoded(2, 2, &[(0, 1, "ali", "Ali"), (0, 2, "veli", "Veli")]);
    assert!(TaskMapping::from_bytes(&good).is_ok());
    let mut version = good.clone();
    version[0] = 2;
    assert_eq!(refused(&version), "it has an unknown version");
    let mut huge_categories = vec![1];
    huge_categories.extend(word(u32::MAX));
    huge_categories.extend([0; 8]);
    assert_eq!(
        refused(&huge_categories),
        "it declares more categories than its bytes hold"
    );
    let mut huge_length = vec![1];
    huge_length.extend(word(1));
    huge_length.extend(word(u32::MAX));
    huge_length.extend(b"PERSON\0\0\0\0\0\0\0\0");
    assert_eq!(refused(&huge_length), "a length runs past its end");
    assert_eq!(
        refused(&encoded(
            3,
            2,
            &[(0, 1, "ali", "Ali"), (0, 2, "veli", "Veli")]
        )),
        "a category's counter differs from its entries"
    );
    assert_eq!(
        refused(&encoded(
            2,
            2,
            &[(0, 1, "ali", "Ali"), (0, 1, "veli", "Veli")]
        )),
        "a token is repeated"
    );
    assert_eq!(
        refused(&encoded(
            2,
            2,
            &[(0, 1, "ali", "Ali"), (0, 2, "ali", "ALI")]
        )),
        "a value is repeated"
    );
    assert_eq!(
        refused(&encoded(
            2,
            2,
            &[(0, 0, "ali", "Ali"), (0, 2, "veli", "Veli")]
        )),
        "a token number is 0 or above its counter"
    );
    assert_eq!(
        refused(&encoded(
            2,
            2,
            &[(0, 1, "ali", "Ali"), (0, 3, "veli", "Veli")]
        )),
        "a token number is 0 or above its counter"
    );
    assert_eq!(
        refused(&encoded(
            2,
            2,
            &[(0, 1, "ali", "Ali"), (1, 2, "veli", "Veli")]
        )),
        "an entry names a category out of range"
    );
    let mut not_text = encoded(1, 1, &[(0, 1, "ali", "Ali")]);
    let last = not_text.len() - 1;
    not_text[last] = 0xFF;
    assert_eq!(refused(&not_text), "a text is not UTF-8");
    let mut trailing = good.clone();
    trailing.push(0);
    assert_eq!(refused(&trailing), "it has trailing bytes");
    let mut bad_name = good.clone();
    bad_name[9] = b'p';
    assert_eq!(refused(&bad_name), "a category name is invalid or repeated");
    let mut two_categories = vec![1];
    two_categories.extend(word(2));
    for _ in 0..2 {
        two_categories.extend(word(1));
        two_categories.extend(b"A");
        two_categories.extend(word(0));
        two_categories.extend(word(0));
    }
    assert_eq!(
        refused(&two_categories),
        "a category name is invalid or repeated"
    );
}
