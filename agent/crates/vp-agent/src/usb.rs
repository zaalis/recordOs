//! USB Direct: a tablet plugged into this Mac with a cable streams over that cable.
//!
//! A web app cannot open a raw USB pipe, so the cable carries IP: the tablet exposes a USB network
//! function (iPad Personal Hotspot over USB, Android USB tethering). This module
//! - detects tablets on the USB bus and the network interfaces backed by a USB function,
//! - keeps the TLS certificate and the mDNS announcement valid for the cable address,
//! - tells iPads connected over Wi-Fi that a cable path exists so they move their session to it,
//! - ends cable sessions immediately when the cable is unplugged so the iPad falls back at once.
//!
//! Sessions that arrive on a cable address only offer that address as ICE candidate: signaling,
//! video and input then never leave the cable.

use crate::agent::Core;
use crate::events::AgentEvent;
use crate::security::Severity;
use crate::session::EndReason;
use serde::Serialize;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const APPLE_VID: u16 = 0x05AC;
/// Fallback rescan interval; device and address notifications normally wake the monitor at once.
const POLL: Duration = Duration::from_secs(3);
/// Coalesces notification bursts (a tablet exposes several USB functions at once).
const SETTLE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TabletKind {
    Ipad,
    Iphone,
    Apple,
    Android,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkCategory {
    Public,
    Private,
    Domain,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FirewallState {
    Allowed,
    Blocked,
    Unknown,
}

/// One present USB device node (a whole device or one of its interfaces).
#[derive(Debug, Clone, Default)]
pub struct UsbEntry {
    pub instance_id: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub interface: Option<u8>,
    /// Friendly name and description, most specific first.
    pub names: Vec<String>,
    pub class: String,
    pub compatible_ids: Vec<String>,
    /// The system reports a problem for this node (typically: no driver installed).
    pub problem: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbTablet {
    pub id: String,
    pub kind: TabletKind,
    pub name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    /// A function of the device has no driver. macOS ships the drivers for iPads and USB
    /// network functions, so this stays false on a Mac.
    pub driver_missing: bool,
}

/// A network interface provided by a USB device, as reported by the platform scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLink {
    pub adapter_id: String,
    pub alias: String,
    pub description: String,
    pub interface_index: u32,
    pub local_ip: Ipv4Addr,
    pub gateway: Option<Ipv4Addr>,
    pub vendor_id: u16,
    pub product_id: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbLink {
    pub id: String,
    pub alias: String,
    pub description: String,
    pub interface_index: u32,
    /// Address of this Mac on the cable.
    pub local_ip: Ipv4Addr,
    /// Address of the tablet on the cable (the tethering gateway).
    pub peer_ip: Option<Ipv4Addr>,
    pub kind: TabletKind,
    pub device_name: String,
    /// Signaling endpoint the tablet uses to reach this Mac through the cable.
    pub url: String,
    pub network: NetworkCategory,
    pub firewall: FirewallState,
    /// macOS currently sends this Mac's Internet traffic through the tablet (its mobile data).
    pub internet: bool,
    pub simulated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbStatus {
    pub enabled: bool,
    pub devices: Vec<UsbTablet>,
    pub links: Vec<UsbLink>,
}

impl UsbStatus {
    pub fn link_for(&self, ip: IpAddr) -> Option<&UsbLink> {
        let ip = match ip {
            IpAddr::V4(v4) => v4,
            IpAddr::V6(v6) => v6.to_ipv4_mapped()?,
        };
        self.links.iter().find(|l| l.local_ip == ip)
    }

    pub fn urls(&self) -> Vec<String> {
        self.links.iter().map(|l| l.url.clone()).collect()
    }

    pub fn local_ips(&self) -> Vec<Ipv4Addr> {
        self.links.iter().map(|l| l.local_ip).collect()
    }
}

/// Test hook: a cable link on a loopback address (debug builds of `vp-agentd --simulate-usb`).
#[derive(Debug, Clone, Copy)]
pub struct Simulation {
    pub ip: Ipv4Addr,
    pub connected: bool,
}

// --- Classification (pure) ----------------------------------------------------------------

/// `USB\VID_05AC&PID_12AB&MI_00\6&1A2B...` → (0x05AC, 0x12AB, Some(0)).
pub fn parse_usb_ids(instance_id: &str) -> Option<(u16, u16, Option<u8>)> {
    let upper = instance_id.to_ascii_uppercase();
    let hardware = upper.strip_prefix("USB\\")?.split('\\').next()?;
    let (mut vid, mut pid, mut interface) = (None, None, None);
    for part in hardware.split('&') {
        if let Some(value) = part.strip_prefix("VID_") {
            vid = u16::from_str_radix(value, 16).ok();
        } else if let Some(value) = part.strip_prefix("PID_") {
            pid = u16::from_str_radix(value, 16).ok();
        } else if let Some(value) = part.strip_prefix("MI_") {
            interface = u8::from_str_radix(value, 16).ok();
        }
    }
    Some((vid?, pid?, interface))
}

/// Apple's iOS/iPadOS device product range (usbmux + PTP + tethering functions).
pub fn is_apple_mobile(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == APPLE_VID && (0x1290..=0x12AF).contains(&product_id)
}

fn has_compatible(entry: &UsbEntry, needle: &str) -> bool {
    entry
        .compatible_ids
        .iter()
        .any(|id| id.to_ascii_uppercase().starts_with(needle))
}

/// True for USB functions that only Android devices expose together: ADB, MTP, RNDIS or NCM
/// tethering. Plain storage, cameras (PTP) and HID never match.
pub fn is_android_function(entry: &UsbEntry) -> bool {
    has_compatible(entry, "USB\\CLASS_FF&SUBCLASS_42&PROT_01")
        || has_compatible(entry, "USB\\MS_COMP_MTP")
        || has_compatible(entry, "USB\\MS_COMP_RNDIS")
        || has_compatible(entry, "USB\\CLASS_E0&SUBCLASS_01&PROT_03")
        || has_compatible(entry, "USB\\CLASS_EF&SUBCLASS_04&PROT_01")
        || has_compatible(entry, "USB\\CLASS_02&SUBCLASS_0D")
}

fn apple_kind(product_id: u16, names: &[&str]) -> TabletKind {
    let joined = names.join(" ").to_lowercase();
    if joined.contains("ipad") {
        TabletKind::Ipad
    } else if joined.contains("iphone") {
        TabletKind::Iphone
    } else {
        match product_id {
            0x12AB => TabletKind::Ipad,
            0x12A8 => TabletKind::Iphone,
            _ => TabletKind::Apple,
        }
    }
}

fn default_name(kind: TabletKind) -> &'static str {
    match kind {
        TabletKind::Ipad => "iPad",
        TabletKind::Iphone => "iPhone",
        TabletKind::Apple => "Appareil Apple",
        TabletKind::Android => "Appareil Android",
    }
}

/// Generic driver/function names that must never be shown as the device name.
fn is_generic_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.is_empty()
        || lower.contains("composite")
        || lower.contains("usb device")
        || lower.contains("périphérique")
        || lower.contains("ethernet")
        || lower.contains("ndis")
        || lower.contains("adb interface")
        || lower.contains("usbncm")
        || lower.contains("mobile device")
}

/// Groups present USB nodes into tablets (one per VID/PID).
pub fn group_tablets(entries: &[UsbEntry]) -> Vec<UsbTablet> {
    let mut groups: Vec<((u16, u16), Vec<&UsbEntry>)> = vec![];
    for entry in entries {
        let key = (entry.vendor_id, entry.product_id);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(entry),
            None => groups.push((key, vec![entry])),
        }
    }
    let mut tablets = vec![];
    for ((vendor_id, product_id), nodes) in groups {
        let names: Vec<&str> = nodes
            .iter()
            .flat_map(|n| n.names.iter().map(String::as_str))
            .collect();
        let kind = if is_apple_mobile(vendor_id, product_id) {
            apple_kind(product_id, &names)
        } else if vendor_id != APPLE_VID && nodes.iter().any(|n| is_android_function(n)) {
            TabletKind::Android
        } else {
            continue;
        };
        // WPD nodes carry the model or the owner's device name ("Galaxy Tab S9", "iPad de Léa").
        let name = nodes
            .iter()
            .filter(|n| n.class.eq_ignore_ascii_case("WPD"))
            .flat_map(|n| n.names.iter())
            .find(|n| !is_generic_name(n))
            .map(|n| n.trim_start_matches("Apple ").trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| default_name(kind).to_string());
        tablets.push(UsbTablet {
            id: format!("{vendor_id:04x}:{product_id:04x}"),
            kind,
            name: crate::util::sanitize_name(&name, 48),
            vendor_id,
            product_id,
            driver_missing: nodes.iter().any(|n| n.problem),
        });
    }
    tablets.sort_by(|a, b| a.id.cmp(&b.id));
    tablets
}

/// Kind of the device behind a cable network interface.
pub fn link_kind(link: &RawLink, tablets: &[UsbTablet]) -> (TabletKind, String) {
    if let Some(tablet) = tablets
        .iter()
        .find(|t| t.vendor_id == link.vendor_id && t.product_id == link.product_id)
    {
        return (tablet.kind, tablet.name.clone());
    }
    let kind = if is_apple_mobile(link.vendor_id, link.product_id) {
        apple_kind(link.product_id, &[])
    } else {
        TabletKind::Android
    };
    (kind, default_name(kind).to_string())
}

pub fn link_url(ip: Ipv4Addr, port: u16) -> String {
    format!("wss://{ip}:{port}/ws")
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub connected: Vec<UsbTablet>,
    pub disconnected: Vec<UsbTablet>,
    pub links_up: Vec<UsbLink>,
    pub links_down: Vec<UsbLink>,
}

pub fn diff(previous: &UsbStatus, next: &UsbStatus) -> Changes {
    let same_link = |a: &UsbLink, b: &UsbLink| a.id == b.id && a.local_ip == b.local_ip;
    Changes {
        connected: next
            .devices
            .iter()
            .filter(|d| !previous.devices.iter().any(|p| p.id == d.id))
            .cloned()
            .collect(),
        disconnected: previous
            .devices
            .iter()
            .filter(|d| !next.devices.iter().any(|n| n.id == d.id))
            .cloned()
            .collect(),
        links_up: next
            .links
            .iter()
            .filter(|l| !previous.links.iter().any(|p| same_link(p, l)))
            .cloned()
            .collect(),
        links_down: previous
            .links
            .iter()
            .filter(|l| !next.links.iter().any(|n| same_link(n, l)))
            .cloned()
            .collect(),
    }
}

// --- Monitor -----------------------------------------------------------------------------

pub enum Wake {
    Changed,
    Stop,
}

/// Background watcher. Owned by the running service; dropping it stops the thread.
pub struct Monitor {
    wake: crossbeam_channel::Sender<Wake>,
    core: Arc<Core>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Monitor {
    pub fn start(core: Arc<Core>) -> Monitor {
        let (wake, receiver) = crossbeam_channel::bounded(32);
        *core.usb_wake.lock() = Some(wake.clone());
        let thread = {
            let core = core.clone();
            let wake = wake.clone();
            std::thread::Builder::new()
                .name("vp-usb".into())
                .spawn(move || run(core, wake, receiver))
                .map_err(|error| tracing::warn!(%error, "USB monitor unavailable"))
                .ok()
        };
        Monitor { wake, core, thread }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.core.usb_wake.lock().take();
        let _ = self.wake.send(Wake::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        *self.core.usb.write() = UsbStatus::default();
    }
}

/// Asks the monitor to rescan now (settings changed, simulated cable toggled).
pub fn wake(core: &Core) {
    if let Some(sender) = core.usb_wake.lock().as_ref() {
        let _ = sender.try_send(Wake::Changed);
    }
}

fn run(
    core: Arc<Core>,
    wake: crossbeam_channel::Sender<Wake>,
    receiver: crossbeam_channel::Receiver<Wake>,
) {
    let _com = crate::platform::usb::ComApartment::enter();
    // Device arrival/removal and address changes wake the loop immediately.
    let _watcher = crate::platform::usb::Watcher::register(wake);
    let mut diagnostics = crate::platform::usb::Diagnostics::default();
    let mut first = true;
    loop {
        if !first {
            match receiver.recv_timeout(POLL) {
                Ok(Wake::Stop) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
                Ok(Wake::Changed) => {
                    let deadline = Instant::now() + SETTLE;
                    loop {
                        match receiver.recv_deadline(deadline) {
                            Ok(Wake::Stop) => return,
                            Ok(Wake::Changed) => continue,
                            Err(_) => break,
                        }
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            }
        }
        first = false;
        if core
            .usb_diagnostics_stale
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            diagnostics = crate::platform::usb::Diagnostics::default();
        }
        crate::agent::refresh_network(&core);
        let status = scan(&core, &mut diagnostics);
        apply(&core, status);
    }
}

fn scan(core: &Core, diagnostics: &mut crate::platform::usb::Diagnostics) -> UsbStatus {
    let (enabled, port) = {
        let settings = core.settings.read();
        (settings.network.usb_direct, settings.network.https_port)
    };
    let mut status = UsbStatus {
        enabled,
        ..UsbStatus::default()
    };
    if enabled {
        let devices = crate::platform::usb::scan_devices();
        let raw = crate::platform::usb::scan_links();
        let categories: HashMap<String, NetworkCategory> = if raw.is_empty() {
            HashMap::new()
        } else {
            crate::platform::usb::network_categories()
        };
        let internet = if raw.is_empty() {
            None
        } else {
            crate::platform::usb::internet_interface()
        };
        status.links = raw
            .iter()
            .map(|link| {
                let (kind, device_name) = link_kind(link, &devices);
                let network = categories
                    .get(&link.adapter_id.to_ascii_uppercase())
                    .copied()
                    .unwrap_or(NetworkCategory::Unknown);
                UsbLink {
                    id: link.adapter_id.clone(),
                    alias: link.alias.clone(),
                    description: link.description.clone(),
                    interface_index: link.interface_index,
                    local_ip: link.local_ip,
                    peer_ip: link.gateway,
                    kind,
                    device_name,
                    url: link_url(link.local_ip, port),
                    network,
                    firewall: diagnostics.firewall(network, port),
                    internet: internet == Some(link.interface_index),
                    simulated: false,
                }
            })
            .collect();
        status.devices = devices;
    }
    if let Some(simulation) = *core.usb_simulation.read() {
        if simulation.connected {
            status.links.push(UsbLink {
                id: "simulated".into(),
                alias: "Simulation".into(),
                description: "Simulated USB link".into(),
                interface_index: 0,
                local_ip: simulation.ip,
                peer_ip: None,
                kind: TabletKind::Ipad,
                device_name: "iPad (simulation)".into(),
                url: format!(
                    "ws://{}:{}/ws",
                    simulation.ip,
                    crate::server::DEV_LOOPBACK_PORT
                ),
                network: NetworkCategory::Private,
                firewall: FirewallState::Allowed,
                internet: false,
                simulated: true,
            });
        }
    }
    status
}

/// Publishes a new USB status and reacts to cable changes.
pub fn apply(core: &Arc<Core>, next: UsbStatus) {
    let previous = {
        let mut current = core.usb.write();
        if *current == next {
            return;
        }
        std::mem::replace(&mut *current, next.clone())
    };
    let changes = diff(&previous, &next);
    for device in &changes.connected {
        let hint = if next.links.iter().any(|l| l.kind == device.kind) {
            String::new()
        } else if device.driver_missing
            && matches!(
                device.kind,
                TabletKind::Ipad | TabletKind::Iphone | TabletKind::Apple
            )
        {
            " Déverrouillez l’iPad et touchez « Se fier » pour activer la liaison par câble.".into()
        } else if device.kind == TabletKind::Android {
            " Activez le partage de connexion USB sur l’appareil pour diffuser par câble.".into()
        } else {
            " Activez le Partage de connexion sur l’iPad pour diffuser par câble.".into()
        };
        core.audit.record(
            Severity::Info,
            "usb.connected",
            format!("{} est branché en USB.{hint}", device.name),
        );
    }
    for device in &changes.disconnected {
        core.audit.record(
            Severity::Info,
            "usb.disconnected",
            format!("{} a été débranché.", device.name),
        );
    }
    for link in &changes.links_down {
        let ended = core
            .sessions
            .end_by_local_ip(&link.local_ip.to_string(), EndReason::LinkLost);
        core.audit.record(
            Severity::Info,
            "usb.link-down",
            format!(
                "Liaison USB avec {} interrompue{}.",
                link.device_name,
                if ended > 0 {
                    " ; la session repasse par le réseau"
                } else {
                    ""
                }
            ),
        );
    }
    for link in &changes.links_up {
        core.audit.record(
            Severity::Notice,
            "usb.link-up",
            format!(
                "Liaison USB directe active avec {} ({}).",
                link.device_name, link.local_ip
            ),
        );
    }
    if !changes.links_up.is_empty() {
        // iPads streaming over Wi-Fi check whether they are the one on the cable and move.
        core.sessions.offer_usb(&next.urls());
    }
    let _ = core.events.send(AgentEvent::UsbChanged);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, names: &[&str], class: &str, compatible: &[&str]) -> UsbEntry {
        let (vendor_id, product_id, interface) = parse_usb_ids(id).unwrap();
        UsbEntry {
            instance_id: id.into(),
            vendor_id,
            product_id,
            interface,
            names: names.iter().map(|s| s.to_string()).collect(),
            class: class.into(),
            compatible_ids: compatible.iter().map(|s| s.to_string()).collect(),
            problem: false,
        }
    }

    #[test]
    fn parses_instance_ids() {
        assert_eq!(
            parse_usb_ids("USB\\VID_05AC&PID_12AB&MI_02\\6&3A5F&0&0002"),
            Some((0x05AC, 0x12AB, Some(2)))
        );
        assert_eq!(
            parse_usb_ids("usb\\vid_04e8&pid_6860\\R52T"),
            Some((0x04E8, 0x6860, None))
        );
        assert_eq!(parse_usb_ids("USB\\ROOT_HUB30\\5&2840EE41&0&0"), None);
        assert_eq!(parse_usb_ids("PCI\\VEN_10EC&DEV_8168"), None);
    }

    #[test]
    fn recognises_an_ipad_and_its_owner_name() {
        let tablets = group_tablets(&[
            entry(
                "USB\\VID_05AC&PID_12AB\\00008103",
                &["Apple Mobile Device USB Composite Device"],
                "USB",
                &[],
            ),
            entry(
                "USB\\VID_05AC&PID_12AB&MI_00\\7&1",
                &["Apple iPad"],
                "WPD",
                &["USB\\Class_06&SubClass_01&Prot_01"],
            ),
            entry(
                "USB\\VID_05AC&PID_12AB&MI_01\\7&2",
                &["Apple Mobile Device USB Device"],
                "USBDevice",
                &["USB\\Class_FF&SubClass_FE&Prot_02"],
            ),
        ]);
        assert_eq!(tablets.len(), 1);
        assert_eq!(tablets[0].kind, TabletKind::Ipad);
        assert_eq!(tablets[0].name, "iPad");
        assert!(!tablets[0].driver_missing);
    }

    #[test]
    fn missing_apple_driver_is_reported() {
        let mut usbmux = entry("USB\\VID_05AC&PID_12AB&MI_01\\7&2", &["iPad"], "", &[]);
        usbmux.problem = true;
        let tablets = group_tablets(&[usbmux]);
        assert!(tablets[0].driver_missing);
    }

    #[test]
    fn recognises_android_but_not_storage_or_cameras() {
        let tablets = group_tablets(&[
            // Samsung T7 SSD: same vendor as Galaxy tablets, but only mass storage.
            entry(
                "USB\\VID_04E8&PID_61FB\\MSFT30S6",
                &["Dispositif de stockage de masse UAS"],
                "SCSIAdapter",
                &["USB\\Class_08&SubClass_06&Prot_62"],
            ),
            // Webcam and a PTP camera.
            entry(
                "USB\\VID_3277&PID_0010&MI_00\\7&D",
                &["Integrated Camera"],
                "Camera",
                &["USB\\Class_0E&SubClass_01"],
            ),
            entry(
                "USB\\VID_04A9&PID_32D1\\1",
                &["Canon EOS"],
                "WPD",
                &["USB\\Class_06&SubClass_01&Prot_01"],
            ),
            // Galaxy Tab in MTP + ADB mode.
            entry(
                "USB\\VID_04E8&PID_6860&MI_00\\8&1",
                &["Galaxy Tab S9"],
                "WPD",
                &["USB\\MS_COMP_MTP", "USB\\Class_06&SubClass_01&Prot_01"],
            ),
            entry(
                "USB\\VID_04E8&PID_6860&MI_01\\8&2",
                &["SAMSUNG Android ADB Interface"],
                "AndroidUsbDeviceClass",
                &["USB\\Class_FF&SubClass_42&Prot_01"],
            ),
        ]);
        assert_eq!(tablets.len(), 1);
        assert_eq!(tablets[0].kind, TabletKind::Android);
        assert_eq!(tablets[0].name, "Galaxy Tab S9");
    }

    #[test]
    fn tethering_only_android_is_recognised() {
        let tablets = group_tablets(&[entry(
            "USB\\VID_18D1&PID_4EE3&MI_00\\1",
            &["Remote NDIS based Internet Sharing Device"],
            "Net",
            &["USB\\Class_E0&SubClass_01&Prot_03"],
        )]);
        assert_eq!(tablets[0].kind, TabletKind::Android);
        assert_eq!(tablets[0].name, "Appareil Android");
    }

    fn link(id: &str, ip: [u8; 4]) -> UsbLink {
        UsbLink {
            id: id.into(),
            alias: "Ethernet 3".into(),
            description: "Apple Mobile Device Ethernet".into(),
            interface_index: 12,
            local_ip: Ipv4Addr::from(ip),
            peer_ip: Some(Ipv4Addr::new(172, 20, 10, 1)),
            kind: TabletKind::Ipad,
            device_name: "iPad".into(),
            url: link_url(Ipv4Addr::from(ip), 47800),
            network: NetworkCategory::Public,
            firewall: FirewallState::Allowed,
            internet: false,
            simulated: false,
        }
    }

    #[test]
    fn diff_reports_cable_changes_and_address_renewals() {
        let before = UsbStatus {
            enabled: true,
            devices: vec![],
            links: vec![link("{A}", [172, 20, 10, 2])],
        };
        let after = UsbStatus {
            enabled: true,
            devices: vec![],
            links: vec![link("{A}", [172, 20, 10, 3])],
        };
        let changes = diff(&before, &after);
        assert_eq!(changes.links_up.len(), 1);
        assert_eq!(changes.links_down.len(), 1);
        assert_eq!(diff(&after, &after), Changes::default());
        // Firewall or category updates are not cable changes.
        let mut blocked = after.clone();
        blocked.links[0].firewall = FirewallState::Blocked;
        assert_eq!(diff(&after, &blocked), Changes::default());
    }

    #[test]
    fn status_finds_the_cable_of_a_connection() {
        let status = UsbStatus {
            enabled: true,
            devices: vec![],
            links: vec![link("{A}", [172, 20, 10, 2])],
        };
        assert!(status.link_for("172.20.10.2".parse().unwrap()).is_some());
        assert!(status
            .link_for("::ffff:172.20.10.2".parse().unwrap())
            .is_some());
        assert!(status.link_for("192.168.1.20".parse().unwrap()).is_none());
        assert_eq!(
            status.urls(),
            vec!["wss://172.20.10.2:47800/ws".to_string()]
        );
    }
}
