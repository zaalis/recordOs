//! USB Direct on macOS: tablets on the USB bus (IOKit) and the network interfaces their cable
//! provides (an iPad's Personal Hotspot appears as "iPad USB", an Android tablet's NCM tethering
//! as a USB Ethernet interface).
//!
//! macOS needs no driver for an iPad's USB network and has no per-network firewall profile, so
//! the Windows-only steps (driver hints, network category, firewall rule) do not apply here.

use super::ffi;
use crate::usb::{FirewallState, NetworkCategory, RawLink, UsbEntry, UsbTablet, Wake};
use std::collections::HashMap;
use std::ffi::{c_char, CString};
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// How often the watcher compares the USB bus and the IPv4 addresses with their last state.
const WATCH_INTERVAL: Duration = Duration::from_millis(700);

/// Nothing to initialise on macOS (kept for parity with the monitor loop).
pub struct ComApartment;

impl ComApartment {
    pub fn enter() -> ComApartment {
        ComApartment
    }
}

// --- Change notifications ------------------------------------------------------------------

/// Wakes the monitor as soon as a USB device or an IPv4 address appears or disappears.
pub struct Watcher {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Watcher {
    pub fn register(wake: crossbeam_channel::Sender<Wake>) -> Watcher {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("vp-usb-watch".into())
                .spawn(move || {
                    let mut last = unsafe { ffi::vp_network_signature() };
                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(WATCH_INTERVAL);
                        let now = unsafe { ffi::vp_network_signature() };
                        if now != last {
                            last = now;
                            let _ = wake.try_send(Wake::Changed);
                        }
                    }
                })
                .map_err(
                    |error| tracing::debug!(%error, "USB change watcher unavailable; polling only"),
                )
                .ok()
        };
        Watcher { stop, thread }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// --- USB devices ---------------------------------------------------------------------------

/// Present USB devices, one entry per device, with their functions as compatible class ids in
/// the same `USB\Class_XX&SubClass_YY&Prot_ZZ` form the shared classifier understands.
pub fn usb_entries() -> Vec<UsbEntry> {
    let mut raw = vec![ffi::vp_usb_device_t::default(); 64];
    let count = unsafe { ffi::vp_usb_devices(raw.as_mut_ptr(), raw.len() as i32) }.clamp(0, 64);
    raw.into_iter()
        .take(count as usize)
        .map(|device| {
            let name = ffi::c_buffer_to_string(&device.name);
            let compatible_ids = device
                .interfaces
                .iter()
                .take(device.interface_count.min(16) as usize)
                .map(|[class, subclass, protocol]| {
                    format!("USB\\Class_{class:02X}&SubClass_{subclass:02X}&Prot_{protocol:02X}")
                })
                .collect();
            UsbEntry {
                instance_id: format!("IOService:{:x}", device.registry_id),
                vendor_id: device.vendor_id,
                product_id: device.product_id,
                interface: None,
                names: if name.is_empty() { vec![] } else { vec![name] },
                // The product string is the model name: let the classifier show it.
                class: "WPD".into(),
                compatible_ids,
                problem: false,
            }
        })
        .collect()
}

/// iPads, iPhones and Android tablets currently on the USB bus.
pub fn scan_devices() -> Vec<UsbTablet> {
    crate::usb::group_tablets(&usb_entries())
}

// --- Cable network interfaces --------------------------------------------------------------

fn interface_usb(name: &str) -> Option<(u16, u16, String)> {
    let bsd = CString::new(name).ok()?;
    let (mut vendor, mut product) = (0u16, 0u16);
    let mut display = [0 as c_char; 128];
    let found = unsafe {
        ffi::vp_interface_usb(
            bsd.as_ptr(),
            &mut vendor,
            &mut product,
            display.as_mut_ptr(),
            display.len(),
        )
    };
    found.then(|| (vendor, product, ffi::c_buffer_to_string(&display)))
}

fn interface_index(name: &str) -> u32 {
    CString::new(name)
        .map(|bsd| unsafe { ffi::vp_interface_index(bsd.as_ptr()) })
        .unwrap_or(0)
}

/// The tethering gateway is the tablet itself: the first host of the cable subnet
/// (172.20.10.1 for an iPad hotspot).
fn gateway_of(ip: Ipv4Addr, netmask: Ipv4Addr) -> Option<Ipv4Addr> {
    let network = u32::from(ip) & u32::from(netmask);
    let gateway = Ipv4Addr::from(network.wrapping_add(1));
    (gateway != ip && u32::from(netmask) != u32::MAX).then_some(gateway)
}

/// Up network interfaces backed by a USB device that carry a private IPv4 address.
pub fn scan_links() -> Vec<RawLink> {
    let mut links: Vec<RawLink> = vec![];
    for interface in if_addrs::get_if_addrs().unwrap_or_default() {
        let if_addrs::IfAddr::V4(v4) = &interface.addr else {
            continue;
        };
        // A link-local address means DHCP from the tablet has not completed yet.
        if !v4.ip.is_private() || links.iter().any(|l| l.adapter_id == interface.name) {
            continue;
        }
        let Some((vendor_id, product_id, display)) = interface_usb(&interface.name) else {
            continue;
        };
        links.push(RawLink {
            adapter_id: interface.name.clone(),
            alias: interface.name.clone(),
            description: display,
            interface_index: interface_index(&interface.name),
            local_ip: v4.ip,
            gateway: gateway_of(v4.ip, v4.netmask),
            vendor_id,
            product_id,
        });
    }
    links.sort_by(|a, b| a.adapter_id.cmp(&b.adapter_id));
    links
}

/// macOS has no Public/Private network profiles: every link reports `Unknown`.
pub fn network_categories() -> HashMap<String, NetworkCategory> {
    HashMap::new()
}

/// Interface that carries this Mac's default route (to warn when it is the tablet's mobile data).
pub fn internet_interface() -> Option<u32> {
    let index = unsafe { ffi::vp_primary_interface_index() };
    (index != 0).then_some(index)
}

// --- Diagnostics ---------------------------------------------------------------------------

/// The application firewall of macOS asks the user once per app and applies to every network,
/// so there is nothing per-link to diagnose.
#[derive(Default)]
pub struct Diagnostics {
    _private: (),
}

impl Diagnostics {
    pub fn firewall(&mut self, _network: NetworkCategory, _port: u16) -> FirewallState {
        FirewallState::Unknown
    }
}

/// No network or firewall change is required for a tablet's USB network on macOS.
pub fn allow_link(_link: &crate::usb::UsbLink) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::gateway_of;
    use std::net::Ipv4Addr;

    #[test]
    fn tablet_is_the_first_host_of_the_cable_subnet() {
        assert_eq!(
            gateway_of(
                Ipv4Addr::new(172, 20, 10, 2),
                Ipv4Addr::new(255, 255, 255, 240)
            ),
            Some(Ipv4Addr::new(172, 20, 10, 1))
        );
        assert_eq!(
            gateway_of(
                Ipv4Addr::new(192, 168, 42, 1),
                Ipv4Addr::new(255, 255, 255, 0)
            ),
            None
        );
    }
}
