//! Network helpers: local-address classification (Local Only enforcement), LAN and Tailscale
//! interface discovery and the stable mDNS hostname used as the web app origin.

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

/// Tailscale (and other WireGuard overlays) number devices in the shared address space
/// 100.64.0.0/10.
pub fn is_overlay_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 100 && (64..128).contains(&b)
}

/// True for peers this PC may serve without Remote Access: the local network, and the private
/// Tailscale network when this PC is itself on one. 100.64.0.0/10 is also carrier-grade NAT
/// space, so it only counts while a local interface has such an address.
pub fn is_private_peer(ip: IpAddr) -> bool {
    if is_local_address(ip) {
        return true;
    }
    let v4 = match ip {
        IpAddr::V4(v4) => v4,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4,
            None => return false,
        },
    };
    is_overlay_v4(v4) && !overlay_addresses().is_empty()
}

/// This PC's Tailscale addresses (cached briefly: checked for every incoming connection).
pub fn overlay_addresses() -> Vec<Ipv4Addr> {
    use parking_lot::Mutex;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<(Instant, Vec<Ipv4Addr>)>> = Mutex::new(None);
    let mut cache = CACHE.lock();
    if let Some((at, list)) = cache.as_ref() {
        if at.elapsed() < Duration::from_secs(5) {
            return list.clone();
        }
    }
    let list: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|iface| match iface.ip() {
            IpAddr::V4(ip) if is_overlay_v4(ip) => Some(ip),
            _ => None,
        })
        .collect();
    *cache = Some((Instant::now(), list.clone()));
    list
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

const VIRTUAL_HINTS: &[&str] = &[
    "vethernet",
    "virtualbox",
    "vmware",
    "hyper-v",
    "wsl",
    "docker",
    "loopback",
    "tailscale",
    "zerotier",
    "vpn",
    "tap",
    "npcap",
    "meta",
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
    list.sort_by_key(|i| (i.virtual_adapter, i.ip.octets()[0] != 192));
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
        assert!(is_overlay_v4("100.101.102.103".parse().unwrap()));
        assert!(!is_overlay_v4("100.128.0.1".parse().unwrap()));
        assert!(!is_overlay_v4("100.63.255.255".parse().unwrap()));
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
