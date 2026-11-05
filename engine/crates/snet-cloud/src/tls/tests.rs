use rustls::NamedGroup;

use super::*;
use crate::test_support::{ANTHROPIC_ENDPOINT, load};

#[test]
fn the_client_offers_tls_1_3_with_the_hybrid_group_first_and_only_http_1_1() {
    let client = client_config(&load(ANTHROPIC_ENDPOINT)).unwrap();
    let groups: Vec<NamedGroup> = client
        .crypto_provider()
        .kx_groups
        .iter()
        .map(|group| group.name())
        .collect();
    assert_eq!(groups.first(), Some(&NamedGroup::X25519MLKEM768));
    assert_eq!(client.alpn_protocols, vec![b"http/1.1".to_vec()]);
    assert!(!client.enable_early_data);
}
