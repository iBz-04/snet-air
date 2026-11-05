use std::sync::Arc;

use rustls::client::Resumption;
use rustls::crypto::aws_lc_rs::{self, kx_group};
use rustls::{ClientConfig, NoKeyLog, RootCertStore};

use crate::config::EndpointConfig;

pub(crate) const HTTP_1_1_ALPN: &[u8] = b"http/1.1";

/// TLS 1.3 only, aws-lc-rs with the X25519MLKEM768 hybrid group first, no
/// resumption, no early data and no key log, verified against the
/// bundled Mozilla roots and the configured certificates without the network
///, offering only `http/1.1`.
pub(crate) fn client_config(config: &EndpointConfig) -> Result<Arc<ClientConfig>, rustls::Error> {
    let mut provider = aws_lc_rs::default_provider();
    provider.kx_groups = vec![
        kx_group::X25519MLKEM768,
        kx_group::X25519,
        kx_group::SECP256R1,
        kx_group::SECP384R1,
    ];

    let mut client = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_root_certificates(root_store(config)?)
        .with_no_client_auth();
    client.alpn_protocols = vec![HTTP_1_1_ALPN.to_vec()];
    client.resumption = Resumption::disabled();
    client.enable_early_data = false;
    client.key_log = Arc::new(NoKeyLog);
    Ok(Arc::new(client))
}

pub(crate) fn root_store(config: &EndpointConfig) -> Result<RootCertStore, rustls::Error> {
    let mut roots = RootCertStore::empty();
    if !config.replace_public_roots {
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }
    for certificate in &config.extra_ca_certificates {
        roots.add(certificate.clone())?;
    }
    Ok(roots)
}

#[cfg(test)]
mod tests;
