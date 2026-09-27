//! Network helpers: local-address classification (Local Only enforcement), LAN interface discovery
//! and the stable mDNS hostname used as the web app origin.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// True for addresses that can only originate from the local network segment or this machine.
pub fn is_local_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_local_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_local_v4(mapped);
            }
            v6.is_loopback() || is_unique_local(v6) || is_link_local_v6(v6)
        }
    }
}

fn is_local_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_private() || ip.is_link_local()
}

fn is_unique_local(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

fn is_link_local_v6(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
}

#[derive(Debug, Clone)]
pub struct LanInterface {
    pub name: String,
    pub ip: Ipv4Addr,
    pub virtual_adapter: bool,
}

/// macOS interface names that are not the Wi-Fi/Ethernet LAN: VPN tunnels (utun, ipsec, ppp),
/// Internet Sharing and VM bridges, AirDrop/AWDL and low-latency WLAN, Apple private links,
/// virtualization hosts and container runtimes.
const VIRTUAL_HINTS: &[&str] = &[
    "utun",
    "ipsec",
    "ppp",
    "bridge",
    "awdl",
    "llw",
    "anpi",
    "ap1",
    "vmnet",
    "vboxnet",
    "docker",
    "lo",
    "gif",
    "stf",
    "tailscale",
    "zerotier",
    "feth",
];

/// Private IPv4 interfaces, physical adapters first.
pub fn lan_interfaces() -> Vec<LanInterface> {
    let mut list: Vec<LanInterface> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|iface| match iface.ip() {
            IpAddr::V4(ip) if ip.is_private() && !ip.is_loopback() => {
                let lower = iface.name.to_lowercase();
                Some(LanInterface {
                    virtual_adapter: VIRTUAL_HINTS.iter().any(|h| lower.contains(h)),
                    name: iface.name.clone(),
                    ip,
                })
            }
            _ => None,
        })
        .collect();
    // Physical interfaces first; en0 (built-in Wi-Fi or Ethernet) before other adapters.
    list.sort_by_key(|i| {
        (
            i.virtual_adapter,
            !i.name.starts_with("en"),
            i.ip.octets()[0] != 192,
        )
    });
    list.dedup_by_key(|i| i.ip);
    list
}

pub fn primary_lan_ip() -> Option<Ipv4Addr> {
    lan_interfaces().into_iter().next().map(|i| i.ip)
}

/// Stable per-installation hostname, e.g. `vp-3fa2c91b.local`.
pub fn mdns_hostname(agent_public_raw: &[u8]) -> String {
    let digest = crate::crypto::sha256(&[b"Vision-Pro/1 hostname", agent_public_raw]);
    format!(
        "vp-{:02x}{:02x}{:02x}{:02x}.local",
        digest[0], digest[1], digest[2], digest[3]
    )
}

pub fn system_hostname() -> String {
    gethostname::gethostname().to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification() {
        for local in [
            "192.168.1.4",
            "10.1.2.3",
            "172.20.0.9",
            "127.0.0.1",
            "169.254.3.3",
            "::1",
            "fe80::1",
            "fd12::3",
            "::ffff:192.168.0.2",
        ] {
            assert!(is_local_address(local.parse().unwrap()), "{local}");
        }
        for remote in [
            "8.8.8.8",
            "100.64.1.1",
            "2001:4860::8888",
            "::ffff:1.1.1.1",
            "172.32.0.1",
        ] {
            assert!(!is_local_address(remote.parse().unwrap()), "{remote}");
        }
    }
}
