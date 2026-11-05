use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// Resolves the host and keeps the addresses in the order returned. Without
/// `allow_private_addresses`, every private, loopback, link-local or reserved address is
/// skipped, so a public name that resolves to a private address is refused.
pub(crate) async fn permitted_addresses(
    host: &str,
    port: u16,
    allow_private_addresses: bool,
) -> std::io::Result<Vec<SocketAddr>> {
    let resolved = tokio::net::lookup_host((host, port)).await?;
    Ok(resolved
        .filter(|address| allow_private_addresses || is_public(address.ip()))
        .collect())
}

pub(crate) fn is_public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_v4(address),
        IpAddr::V6(address) => match address.to_ipv4_mapped() {
            Some(mapped) => is_public_v4(mapped),
            None => is_public_v6(address),
        },
    }
}

fn is_public_v4(address: Ipv4Addr) -> bool {
    let [first, second, ..] = address.octets();
    let is_this_network = first == 0;
    let is_shared = first == 100 && (64..=127).contains(&second);
    !(address.is_loopback()
        || address.is_private()
        || is_shared
        || is_this_network
        || address.is_link_local()
        || address.is_unspecified()
        || address.is_multicast())
}

fn is_public_v6(address: Ipv6Addr) -> bool {
    let first_segment = address.segments()[0];
    let is_link_local = first_segment & 0xffc0 == 0xfe80;
    let is_unique_local = first_segment & 0xfe00 == 0xfc00;
    !(address.is_loopback()
        || address.is_unspecified()
        || address.is_multicast()
        || is_link_local
        || is_unique_local)
}

#[cfg(test)]
mod tests;
