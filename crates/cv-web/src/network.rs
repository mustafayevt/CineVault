//! Which addresses count as the home network, and the computer's own addresses.
//!
//! "Home network" means the same computer or a network that is not routed on the internet:
//! loopback, the private ranges, link-local and carrier-grade NAT (which some VPN meshes use).
//! The welcome page, the password reset and the desktop's "Only devices on my home network"
//! guard all use this one rule.

use std::net::{IpAddr, Ipv4Addr, UdpSocket};

/// True for loopback, `10/8`, `172.16/12`, `192.168/16`, `169.254/16`, `100.64/10`, `::1`,
/// `fc00::/7`, `fe80::/10`, and the IPv4-mapped IPv6 forms of the IPv4 ranges.
#[must_use]
pub fn is_home_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_home_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_home_v4(v4);
            }
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

fn is_home_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || (a == 100 && (64..128).contains(&b))
}

/// Adapter names that belong to virtual machines, containers or VPN plumbing rather than the
/// network a phone or a TV is on.
const VIRTUAL_ADAPTERS: &[&str] = &[
    "vethernet",
    "wsl",
    "hyper-v",
    "docker",
    "veth",
    "virbr",
    "br-",
    "virtualbox",
    "vmware",
    "vmnet",
    "vboxnet",
    "tailscale",
    "zerotier",
    "utun",
];

/// The computer's IPv4 addresses another device on the home network can open: the address of
/// the default route first, then the other private addresses of real adapters. Loopback,
/// link-local and virtual adapters are left out.
#[must_use]
pub fn lan_addresses() -> Vec<Ipv4Addr> {
    let mut found: Vec<Ipv4Addr> = Vec::new();
    if let Some(primary) = primary_address() {
        found.push(primary);
    }
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        let mut others: Vec<Ipv4Addr> = interfaces
            .iter()
            .filter(|interface| {
                let name = interface.name.to_ascii_lowercase();
                !VIRTUAL_ADAPTERS.iter().any(|marker| name.contains(marker))
            })
            .filter_map(|interface| match interface.ip() {
                IpAddr::V4(v4) if v4.is_private() => Some(v4),
                _ => None,
            })
            .collect();
        others.sort_by_key(|ip| rank(*ip));
        for ip in others {
            if !found.contains(&ip) {
                found.push(ip);
            }
        }
    }
    found
}

/// `192.168/16` first, then `10/8`, then `172.16/12`: the order home routers use most.
fn rank(ip: Ipv4Addr) -> u8 {
    match ip.octets() {
        [192, 168, ..] => 0,
        [10, ..] => 1,
        _ => 2,
    }
}

/// The local address of the default route: a UDP socket "connected" to a public address picks
/// the outgoing interface without sending anything.
fn primary_address() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(v4) if v4.is_private() || (v4.octets()[0] == 100) => Some(v4),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn home_addresses_are_recognised() {
        for text in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.254",
            "192.168.1.20",
            "169.254.10.10",
            "100.64.0.1",
            "100.127.255.255",
            "::1",
            "fd12:3456::1",
            "fc00::1",
            "fe80::1",
            "::ffff:192.168.1.20",
        ] {
            assert!(is_home_address(ip(text)), "{text}");
        }
    }

    #[test]
    fn public_addresses_are_not_home() {
        for text in [
            "8.8.8.8",
            "172.32.0.1",
            "100.128.0.1",
            "100.63.255.255",
            "192.169.0.1",
            "2001:db8::1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert!(!is_home_address(ip(text)), "{text}");
        }
    }

    #[test]
    fn the_address_list_holds_only_private_ipv4() {
        for address in lan_addresses() {
            assert!(address.is_private() || address.octets()[0] == 100);
            assert!(!address.is_loopback());
        }
    }

    #[test]
    fn private_addresses_are_ordered_like_home_routers() {
        let mut list = [
            Ipv4Addr::new(172, 20, 0, 2),
            Ipv4Addr::new(10, 0, 0, 2),
            Ipv4Addr::new(192, 168, 1, 2),
        ];
        list.sort_by_key(|ip| rank(*ip));
        assert_eq!(list[0], Ipv4Addr::new(192, 168, 1, 2));
        assert_eq!(list[2], Ipv4Addr::new(172, 20, 0, 2));
    }
}
