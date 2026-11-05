use super::{Builtin, Source, canonical, detect_builtin};
use crate::resolve::merge;

/// The spans one built-in finds in `text`, merged as resolution would.
fn spans(kind: Builtin, text: &str) -> Vec<String> {
    let mut found = Vec::new();
    detect_builtin(kind, text, &mut found);
    merge(found)
        .into_iter()
        .map(|span| text[span.start..span.end].to_string())
        .collect()
}

fn finds(kind: Builtin, text: &str, expected: &str) {
    assert_eq!(spans(kind, text), vec![expected.to_string()], "{text}");
}

fn misses(kind: Builtin, text: &str) {
    assert!(
        spans(kind, text).is_empty(),
        "{text}: {:?}",
        spans(kind, text)
    );
}

fn canonical_of(kind: Builtin, text: &str) -> String {
    canonical(Source::Builtin(kind), text).unwrap().to_string()
}

#[test]
fn email_addresses() {
    finds(
        Builtin::Email,
        "Write to ali.veli@example.com.",
        "ali.veli@example.com",
    );
    finds(Builtin::Email, "see...ali@example.com", "ali@example.com");
    finds(
        Builtin::Email,
        "ayşe@örnek.com.tr bekliyor",
        "ayşe@örnek.com.tr",
    );
    finds(
        Builtin::Email,
        "(ops+night@plant-3.example.org)",
        "ops+night@plant-3.example.org",
    );
    misses(Builtin::Email, "a@localhost");
    misses(Builtin::Email, "user@host.c0m");
    // Every start is tried, so the valid address after `..` is found.
    finds(Builtin::Email, "a..b@example.com", "b@example.com");
    misses(Builtin::Email, "ali@example.com-x");
    assert_eq!(
        canonical_of(Builtin::Email, "Ali@Example.COM"),
        "ali@example.com"
    );
}

#[test]
fn phone_numbers() {
    for text in [
        "+90 532 123 45 67",
        "+90-532-123-45-67",
        "+905321234567",
        "0090 532 123 45 67",
    ] {
        finds(Builtin::Phone, &format!("Call {text} now"), text);
    }
    for text in [
        "0532 123 45 67",
        "(0532) 123 45 67",
        "0 532 123 45 67",
        "0532-123-45-67",
        "05321234567",
    ] {
        finds(Builtin::Phone, &format!("Ara: {text}."), text);
    }
    finds(
        Builtin::Phone,
        "+44 (0) 20 7946 0958",
        "+44 (0) 20 7946 0958",
    );
    finds(
        Builtin::Phone,
        "+90 532 123 45 67 2026 raporu",
        "+90 532 123 45 67",
    );
    misses(Builtin::Phone, "+1234567890123456");
    misses(Builtin::Phone, "reading +12345.678 bar");
    misses(Builtin::Phone, "01234567890");
    misses(Builtin::Phone, "9+90 532 123 45 67");
    assert_eq!(
        canonical_of(Builtin::Phone, "0532 123 45 67"),
        "+905321234567"
    );
    assert_eq!(
        canonical_of(Builtin::Phone, "0090 532 123 45 67"),
        "+905321234567"
    );
    assert_eq!(
        canonical_of(Builtin::Phone, "+44 (0) 20 7946 0958"),
        canonical_of(Builtin::Phone, "+44 20 7946 0958")
    );
}

#[test]
fn ibans() {
    for text in [
        "DE89 3704 0044 0532 0130 00",
        "DE89370400440532013000",
        "GB82 WEST 1234 5698 7654 32",
        "TR33 0006 1005 1978 6457 8413 26",
    ] {
        finds(Builtin::Iban, &format!("IBAN: {text}."), text);
    }
    finds(
        Builtin::Iban,
        "ES91 2100 0418 4502 0005 1332 EUR",
        "ES91 2100 0418 4502 0005 1332",
    );
    misses(Builtin::Iban, "de89 3704 0044 0532 0130 00");
    misses(Builtin::Iban, "DE88 3704 0044 0532 0130 00");
    misses(Builtin::Iban, "XDE89370400440532013000");
    assert_eq!(
        canonical_of(Builtin::Iban, "DE89 3704 0044 0532 0130 00"),
        "DE89370400440532013000"
    );
}

#[test]
fn payment_cards() {
    for text in [
        "4111 1111 1111 1111",
        "4111-1111-1111-1111",
        "4111111111111111",
        "3782 822463 10005",
    ] {
        finds(Builtin::PaymentCard, &format!("card {text} ok"), text);
    }
    finds(
        Builtin::PaymentCard,
        "4111 1111 1111 1111 123",
        "4111 1111 1111 1111",
    );
    misses(Builtin::PaymentCard, "4111 1111-1111 1111");
    misses(Builtin::PaymentCard, "stamp 1730800000000");
    misses(Builtin::PaymentCard, "GTIN 40012345678901");
    misses(Builtin::PaymentCard, "41111111111111115");
    finds(
        Builtin::PaymentCard,
        "x.4111111111111111",
        "4111111111111111",
    );
    misses(Builtin::PaymentCard, "1.4111111111111111");
    // A stated false positive: a Luhn-valid GTIN-14 with a card prefix.
    finds(
        Builtin::PaymentCard,
        "GTIN 40012345678905",
        "40012345678905",
    );
    assert_eq!(
        canonical_of(Builtin::PaymentCard, "4111-1111-1111-1111"),
        "4111111111111111"
    );
}

#[test]
fn turkish_identity_numbers() {
    finds(Builtin::TrIdentity, "TCKN 10000000146.", "10000000146");
    finds(Builtin::TrIdentity, "12345678950", "12345678950");
    misses(Builtin::TrIdentity, "12345678901");
    misses(Builtin::TrIdentity, "02345678950");
    misses(Builtin::TrIdentity, "123456789501");
    misses(Builtin::TrIdentity, "7.12345678950");
    misses(Builtin::TrIdentity, "12345678950.5");
}

#[test]
fn ip_addresses() {
    finds(
        Builtin::IpAddress,
        "host 192.168.1.20/24 up",
        "192.168.1.20",
    );
    finds(Builtin::IpAddress, "192.168.1.20:8080", "192.168.1.20");
    misses(Builtin::IpAddress, "10.0.0.256");
    misses(Builtin::IpAddress, "01.2.3.4");
    misses(Builtin::IpAddress, "1.2.3.4.5");
    // A stated false positive: a four-part version string.
    finds(Builtin::IpAddress, "firmware 2.10.0.1 loaded", "2.10.0.1");
    finds(Builtin::IpAddress, "fe80::1%eth0", "fe80::1");
    finds(Builtin::IpAddress, "net 2001:db8::1/64", "2001:db8::1");
    finds(Builtin::IpAddress, "[::1]:443", "::1");
    finds(Builtin::IpAddress, "ip:2001:db8::1", "2001:db8::1");
    finds(Builtin::IpAddress, "2001:db8::1: refused", "2001:db8::1");
    finds(Builtin::IpAddress, "at fe80::1.", "fe80::1");
    misses(Builtin::IpAddress, "std::vector<int>");
    misses(Builtin::IpAddress, "at 12:30:45 today");
    misses(Builtin::IpAddress, "dead::beef");
    assert_eq!(
        canonical_of(Builtin::IpAddress, "2001:0db8:0:0:0:0:0:1"),
        "2001:db8::1"
    );
}

#[test]
fn mac_addresses() {
    for text in [
        "00:1A:2B:3C:4D:5E",
        "00-1a-2b-3c-4d-5e",
        "001a.2b3c.4d5e",
        "00:1A:2B:3C:4D:5E:6F:70",
    ] {
        finds(Builtin::MacAddress, &format!("mac {text} seen"), text);
    }
    finds(
        Builtin::MacAddress,
        "MAC:00:1A:2B:3C:4D:5E",
        "00:1A:2B:3C:4D:5E",
    );
    misses(Builtin::MacAddress, "00:1A-2B:3C:4D:5E");
    misses(Builtin::MacAddress, "x00:1A:2B:3C:4D:5E");
    assert_eq!(
        canonical_of(Builtin::MacAddress, "00-1A-2B-3C-4D-5E"),
        "001a2b3c4d5e"
    );
}
