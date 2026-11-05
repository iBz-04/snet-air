use std::net::IpAddr;

use super::is_public;

fn public(text: &str) -> bool {
    is_public(text.parse::<IpAddr>().unwrap())
}

#[test]
fn ipv4_classes_of_cld_7_are_refused() {
    for refused in [
        "127.0.0.1",
        "127.255.255.254",
        "10.0.0.1",
        "172.16.0.1",
        "172.31.255.255",
        "192.168.1.1",
        "100.64.0.1",
        "100.127.255.255",
        "0.0.0.0",
        "0.1.2.3",
        "169.254.10.10",
        "224.0.0.1",
        "239.255.255.255",
    ] {
        assert!(!public(refused), "{refused} must be refused");
    }
}

#[test]
fn ipv4_public_addresses_and_class_edges_are_allowed() {
    for allowed in [
        "8.8.8.8",
        "1.1.1.1",
        "100.63.255.255",
        "100.128.0.0",
        "172.15.255.255",
        "172.32.0.0",
        "192.167.255.255",
        "169.253.255.255",
        "223.255.255.255",
    ] {
        assert!(public(allowed), "{allowed} must be allowed");
    }
}

#[test]
fn ipv6_classes_of_cld_7_are_refused() {
    for refused in [
        "::1", "::", "fe80::1", "febf::1", "fc00::1", "fdff::1", "ff02::1",
    ] {
        assert!(!public(refused), "{refused} must be refused");
    }
}

#[test]
fn ipv4_mapped_ipv6_is_judged_by_its_ipv4_form() {
    for refused in [
        "::ffff:127.0.0.1",
        "::ffff:10.0.0.1",
        "::ffff:100.64.0.1",
        "::ffff:0.0.0.1",
        "::ffff:169.254.1.1",
        "::ffff:192.168.0.1",
    ] {
        assert!(!public(refused), "{refused} must be refused");
    }
    assert!(public("::ffff:8.8.8.8"));
}

#[test]
fn ipv6_public_addresses_are_allowed() {
    for allowed in ["2001:4860:4860::8888", "2606:4700::1111", "fec0::1"] {
        assert!(public(allowed), "{allowed} must be allowed");
    }
}
