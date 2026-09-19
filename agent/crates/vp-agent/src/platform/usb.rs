//! Windows side of USB Direct: present USB devices (SetupAPI), network interfaces backed by a USB
//! function, their Windows network category and firewall state, change notifications, and the
//! consented (UAC) fix that lets a tablet reach this PC through its cable.

use crate::usb::{FirewallState, NetworkCategory, RawLink, UsbEntry, UsbLink, UsbTablet, Wake};
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};
use windows::core::{Interface, BSTR, GUID, HRESULT, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::*;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_BUFFER_OVERFLOW, HANDLE, HWND, NO_ERROR, WAIT_OBJECT_0,
};
use windows::Win32::NetworkManagement::IpHelper::*;
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::NetworkManagement::WindowsFirewall::*;
use windows::Win32::Networking::NetworkListManager::*;
use windows::Win32::Networking::WinSock::{AF_INET, AF_UNSPEC, SOCKADDR_IN, SOCKET_ADDRESS};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Ole::IEnumVARIANT;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::System::Variant::{VARIANT, VT_DISPATCH, VT_UNKNOWN};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NO_CONSOLE, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

/// {A5DCBF10-6530-11D2-901F-00C04FB951ED}: every USB device.
const GUID_DEVINTERFACE_USB_DEVICE: GUID = GUID::from_u128(0xa5dcbf10_6530_11d2_901f_00c04fb951ed);
/// Network adapter class; each adapter has `Connection\PnPInstanceId` below it.
const NETWORK_CLASS_KEY: &str =
    "SYSTEM\\CurrentControlSet\\Control\\Network\\{4D36E972-E325-11CE-BFC1-08002BE10318}";
const FIREWALL_CACHE: Duration = Duration::from_secs(20);

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(words: &[u16]) -> Vec<String> {
    words
        .split(|&w| w == 0)
        .map(String::from_utf16_lossy)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn guid_string(guid: &GUID) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4[0],
        guid.data4[1],
        guid.data4[2],
        guid.data4[3],
        guid.data4[4],
        guid.data4[5],
        guid.data4[6],
        guid.data4[7]
    )
}

/// COM apartment for the monitor thread (network list and firewall queries).
pub struct ComApartment(bool);

impl ComApartment {
    pub fn enter() -> ComApartment {
        ComApartment(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

// --- Change notifications ------------------------------------------------------------------

/// USB arrival/removal and unicast address changes wake the monitor immediately.
pub struct Watcher {
    device: Option<HCMNOTIFICATION>,
    address: Option<HANDLE>,
    context: *mut crossbeam_channel::Sender<Wake>,
}

impl Watcher {
    pub fn register(wake: crossbeam_channel::Sender<Wake>) -> Watcher {
        let context = Box::into_raw(Box::new(wake));
        let mut watcher = Watcher {
            device: None,
            address: None,
            context,
        };
        unsafe {
            let mut filter = CM_NOTIFY_FILTER {
                cbSize: std::mem::size_of::<CM_NOTIFY_FILTER>() as u32,
                FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
                ..Default::default()
            };
            filter.u.DeviceInterface.ClassGuid = GUID_DEVINTERFACE_USB_DEVICE;
            let mut handle = HCMNOTIFICATION::default();
            if CM_Register_Notification(
                &filter,
                Some(context as *const _),
                Some(on_device),
                &mut handle,
            ) == CR_SUCCESS
            {
                watcher.device = Some(handle);
            } else {
                tracing::debug!("USB device notifications unavailable; polling only");
            }
            let mut handle = HANDLE::default();
            if NotifyUnicastIpAddressChange(
                AF_UNSPEC,
                Some(on_address),
                Some(context as *const _),
                false,
                &mut handle,
            ) == NO_ERROR
            {
                watcher.address = Some(handle);
            } else {
                tracing::debug!("address notifications unavailable; polling only");
            }
        }
        watcher
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        unsafe {
            // Both calls wait for callbacks in progress, so the context can be freed afterwards.
            if let Some(handle) = self.device.take() {
                let _ = CM_Unregister_Notification(handle);
            }
            if let Some(handle) = self.address.take() {
                let _ = CancelMibChangeNotify2(handle);
            }
            drop(Box::from_raw(self.context));
        }
    }
}

unsafe fn nudge(context: *const core::ffi::c_void) {
    if let Some(wake) = unsafe { (context as *const crossbeam_channel::Sender<Wake>).as_ref() } {
        let _ = wake.try_send(Wake::Changed);
    }
}

unsafe extern "system" fn on_device(
    _notify: HCMNOTIFICATION,
    context: *const core::ffi::c_void,
    _action: CM_NOTIFY_ACTION,
    _data: *const CM_NOTIFY_EVENT_DATA,
    _size: u32,
) -> u32 {
    unsafe { nudge(context) };
    0
}

unsafe extern "system" fn on_address(
    context: *const core::ffi::c_void,
    _row: *const MIB_UNICASTIPADDRESS_ROW,
    _kind: MIB_NOTIFICATION_TYPE,
) {
    unsafe { nudge(context) };
}

// --- USB devices ---------------------------------------------------------------------------

unsafe fn device_property(
    set: HDEVINFO,
    data: &SP_DEVINFO_DATA,
    property: SETUP_DI_REGISTRY_PROPERTY,
) -> Vec<String> {
    unsafe {
        let mut required = 0u32;
        let _ =
            SetupDiGetDeviceRegistryPropertyW(set, data, property, None, None, Some(&mut required));
        if !(2..=64 * 1024).contains(&required) {
            return vec![];
        }
        let mut buffer = vec![0u8; required as usize];
        if SetupDiGetDeviceRegistryPropertyW(set, data, property, None, Some(&mut buffer), None)
            .is_err()
        {
            return vec![];
        }
        let words: Vec<u16> = buffer
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        from_wide(&words)
    }
}

unsafe fn instance_id(set: HDEVINFO, data: &SP_DEVINFO_DATA) -> Option<String> {
    let mut buffer = [0u16; 512];
    unsafe { SetupDiGetDeviceInstanceIdW(set, data, Some(&mut buffer), None).ok()? };
    from_wide(&buffer).into_iter().next()
}

/// Every present node enumerated by the USB bus driver.
pub fn usb_entries() -> Vec<UsbEntry> {
    let mut entries = vec![];
    unsafe {
        let Ok(set) = SetupDiGetClassDevsW(
            None,
            windows::core::w!("USB"),
            None,
            DIGCF_PRESENT | DIGCF_ALLCLASSES,
        ) else {
            return entries;
        };
        let mut index = 0u32;
        loop {
            let mut data = SP_DEVINFO_DATA {
                cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInfo(set, index, &mut data).is_err() {
                break;
            }
            index += 1;
            if index > 1024 {
                break;
            }
            let Some(id) = instance_id(set, &data) else {
                continue;
            };
            let Some((vendor_id, product_id, interface)) = crate::usb::parse_usb_ids(&id) else {
                continue;
            };
            let mut names = device_property(set, &data, SPDRP_FRIENDLYNAME);
            names.extend(device_property(set, &data, SPDRP_DEVICEDESC));
            let mut status = CM_DEVNODE_STATUS_FLAGS(0);
            let mut problem = CM_PROB(0);
            let has_problem = CM_Get_DevNode_Status(&mut status, &mut problem, data.DevInst, 0)
                == CR_SUCCESS
                && problem.0 != 0;
            entries.push(UsbEntry {
                instance_id: id,
                vendor_id,
                product_id,
                interface,
                names,
                class: device_property(set, &data, SPDRP_CLASS)
                    .into_iter()
                    .next()
                    .unwrap_or_default(),
                compatible_ids: device_property(set, &data, SPDRP_COMPATIBLEIDS),
                problem: has_problem,
            });
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    entries
}

pub fn scan_devices() -> Vec<UsbTablet> {
    crate::usb::group_tablets(&usb_entries())
}

// --- USB network interfaces ----------------------------------------------------------------

fn pnp_instance(adapter_id: &str) -> Option<String> {
    let key = wide(&format!("{NETWORK_CLASS_KEY}\\{adapter_id}\\Connection"));
    let value = wide("PnPInstanceId");
    let mut buffer = [0u16; 512];
    let mut size = (buffer.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if result != NO_ERROR {
        return None;
    }
    from_wide(&buffer).into_iter().next()
}

unsafe fn ipv4_of(address: &SOCKET_ADDRESS) -> Option<Ipv4Addr> {
    unsafe {
        if address.lpSockaddr.is_null() || (*address.lpSockaddr).sa_family != AF_INET {
            return None;
        }
        let v4 = &*(address.lpSockaddr as *const SOCKADDR_IN);
        let b = v4.sin_addr.S_un.S_un_b;
        Some(Ipv4Addr::new(b.s_b1, b.s_b2, b.s_b3, b.s_b4))
    }
}

/// Up network interfaces whose device is a USB function and that carry a private IPv4 address.
pub fn scan_links() -> Vec<RawLink> {
    let mut links = vec![];
    let flags = GAA_FLAG_INCLUDE_GATEWAYS
        | GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER;
    // u64 storage keeps the adapter structures correctly aligned.
    let mut buffer: Vec<u64> = vec![0; 2048];
    let mut size = (buffer.len() * 8) as u32;
    unsafe {
        let mut attempts = 0;
        loop {
            let result = GetAdaptersAddresses(
                AF_INET.0 as u32,
                flags,
                None,
                Some(buffer.as_mut_ptr().cast()),
                &mut size,
            );
            if result == ERROR_BUFFER_OVERFLOW.0 && attempts < 4 {
                attempts += 1;
                buffer = vec![0; (size as usize).div_ceil(8) + 64];
                size = (buffer.len() * 8) as u32;
                continue;
            }
            if result != NO_ERROR.0 {
                return links;
            }
            break;
        }
        let mut adapter = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while let Some(current) = adapter.as_ref() {
            adapter = current.Next;
            if current.OperStatus != IfOperStatusUp {
                continue;
            }
            let Ok(adapter_id) = current.AdapterName.to_string() else {
                continue;
            };
            let Some(pnp) = pnp_instance(&adapter_id) else {
                continue;
            };
            let Some((vendor_id, product_id, _)) = crate::usb::parse_usb_ids(&pnp) else {
                continue;
            };
            let mut local_ip = None;
            let mut unicast = current.FirstUnicastAddress;
            while let Some(entry) = unicast.as_ref() {
                unicast = entry.Next;
                if let Some(ip) = ipv4_of(&entry.Address) {
                    // A link-local address means DHCP from the tablet has not completed yet.
                    if ip.is_private() {
                        local_ip = Some(ip);
                        break;
                    }
                }
            }
            let Some(local_ip) = local_ip else { continue };
            let gateway = current
                .FirstGatewayAddress
                .as_ref()
                .and_then(|g| ipv4_of(&g.Address));
            links.push(RawLink {
                adapter_id: adapter_id.to_ascii_uppercase(),
                alias: current.FriendlyName.to_string().unwrap_or_default(),
                description: current.Description.to_string().unwrap_or_default(),
                interface_index: current.Anonymous1.Anonymous.IfIndex,
                local_ip,
                gateway,
                vendor_id,
                product_id,
            });
        }
    }
    links.sort_by(|a, b| a.adapter_id.cmp(&b.adapter_id));
    links
}

/// Windows network category (Public/Private) of each connected adapter, keyed by adapter GUID.
pub fn network_categories() -> HashMap<String, NetworkCategory> {
    let mut categories = HashMap::new();
    unsafe {
        let Ok(manager) =
            CoCreateInstance::<_, INetworkListManager>(&NetworkListManager, None, CLSCTX_ALL)
        else {
            return categories;
        };
        let Ok(connections) = manager.GetNetworkConnections() else {
            return categories;
        };
        for _ in 0..64 {
            let mut item = [None];
            let mut fetched = 0u32;
            if connections.Next(&mut item, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(connection) = item[0].take() else {
                break;
            };
            let (Ok(adapter), Ok(network)) = (connection.GetAdapterId(), connection.GetNetwork())
            else {
                continue;
            };
            let category = match network.GetCategory() {
                Ok(NLM_NETWORK_CATEGORY_PRIVATE) => NetworkCategory::Private,
                Ok(NLM_NETWORK_CATEGORY_DOMAIN_AUTHENTICATED) => NetworkCategory::Domain,
                Ok(NLM_NETWORK_CATEGORY_PUBLIC) => NetworkCategory::Public,
                _ => NetworkCategory::Unknown,
            };
            categories.insert(guid_string(&adapter), category);
        }
    }
    categories
}

/// Interface Windows uses to reach the Internet (IPv4), if any.
pub fn internet_interface() -> Option<u32> {
    let mut index = 0u32;
    // 1.1.1.1 in network byte order; only the routing decision is queried, nothing is sent.
    let destination = u32::from_ne_bytes([1, 1, 1, 1]);
    (unsafe { GetBestInterface(destination, &mut index) } == NO_ERROR.0).then_some(index)
}

// --- Firewall ------------------------------------------------------------------------------

#[derive(Default)]
pub struct Diagnostics {
    firewall: HashMap<(i32, u16), (Instant, FirewallState)>,
}

impl Diagnostics {
    /// Whether Windows Defender Firewall lets this program accept connections on networks of
    /// the given category. Cached briefly: rule enumeration costs a few milliseconds.
    pub fn firewall(&mut self, network: NetworkCategory, port: u16) -> FirewallState {
        let profile = match network {
            NetworkCategory::Public => NET_FW_PROFILE2_PUBLIC.0,
            NetworkCategory::Private => NET_FW_PROFILE2_PRIVATE.0,
            NetworkCategory::Domain => NET_FW_PROFILE2_DOMAIN.0,
            NetworkCategory::Unknown => return FirewallState::Unknown,
        };
        if let Some((at, state)) = self.firewall.get(&(profile, port)) {
            if at.elapsed() < FIREWALL_CACHE {
                return *state;
            }
        }
        let state = firewall_state(profile, port).unwrap_or(FirewallState::Unknown);
        self.firewall
            .insert((profile, port), (Instant::now(), state));
        state
    }
}

fn normalize_path(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path.trim().trim_matches('"');
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                out.push_str(&std::env::var(name).unwrap_or_else(|_| format!("%{name}%")));
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.replace('/', "\\").to_lowercase()
}

/// True if a firewall port list ("47800", "80,443", "47000-48000", "*") includes `port`.
pub fn port_listed(list: &str, port: u16) -> bool {
    list.split(',').map(str::trim).any(|item| {
        if item == "*" {
            return true;
        }
        match item.split_once('-') {
            Some((low, high)) => matches!((low.trim().parse::<u16>(), high.trim().parse::<u16>()), (Ok(l), Ok(h)) if (l..=h).contains(&port)),
            None => item.parse::<u16>() == Ok(port),
        }
    })
}

unsafe fn rule_of(variant: &VARIANT) -> Option<INetFwRule> {
    unsafe {
        let inner = &variant.Anonymous.Anonymous;
        if inner.vt == VT_DISPATCH {
            (*inner.Anonymous.pdispVal).as_ref()?.cast().ok()
        } else if inner.vt == VT_UNKNOWN {
            (*inner.Anonymous.punkVal).as_ref()?.cast().ok()
        } else {
            None
        }
    }
}

fn firewall_state(profile: i32, port: u16) -> windows::core::Result<FirewallState> {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    firewall_state_for(&exe, profile, port)
}

fn firewall_state_for(exe: &str, profile: i32, port: u16) -> windows::core::Result<FirewallState> {
    let exe = normalize_path(exe);
    unsafe {
        let policy: INetFwPolicy2 = CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER)?;
        let kind = NET_FW_PROFILE_TYPE2(profile);
        if !policy.get_FirewallEnabled(kind)?.as_bool() {
            return Ok(FirewallState::Allowed);
        }
        let default_allow = policy.get_DefaultInboundAction(kind)? == NET_FW_ACTION_ALLOW;
        let rules = policy.Rules()?;
        let enumerator: IEnumVARIANT = rules._NewEnum()?.cast()?;
        let (mut allowed, mut blocked) = (default_allow, false);
        for _ in 0..20_000 {
            let mut items = [VARIANT::default()];
            let mut fetched = 0u32;
            let result: HRESULT = enumerator.Next(&mut items, &mut fetched);
            if result.is_err() || fetched == 0 {
                break;
            }
            let Some(rule) = rule_of(&items[0]) else {
                continue;
            };
            let applies = rule.Direction().ok() == Some(NET_FW_RULE_DIR_IN)
                && rule.Enabled().map(|e| e.as_bool()).unwrap_or(false)
                && rule.Profiles().map(|p| p & profile != 0).unwrap_or(false);
            if !applies {
                continue;
            }
            let application = rule
                .ApplicationName()
                .map(|b: BSTR| b.to_string())
                .unwrap_or_default();
            let relevant = if application.is_empty() {
                // Program-independent rules only count when they name our port explicitly.
                let protocol = rule.Protocol().unwrap_or(0);
                let ports = rule.LocalPorts().map(|b| b.to_string()).unwrap_or_default();
                (protocol == 6 || protocol == 256)
                    && !ports.is_empty()
                    && ports != "*"
                    && port_listed(&ports, port)
            } else {
                !exe.is_empty() && normalize_path(&application) == exe
            };
            if !relevant {
                continue;
            }
            match rule.Action() {
                Ok(NET_FW_ACTION_BLOCK) => blocked = true,
                Ok(NET_FW_ACTION_ALLOW) => allowed = true,
                _ => {}
            }
        }
        Ok(if blocked {
            FirewallState::Blocked
        } else if allowed {
            FirewallState::Allowed
        } else {
            FirewallState::Blocked
        })
    }
}

/// With the user's consent (UAC), marks the cable network as private, allows Vision-Pro on it and
/// gives it a high route metric, so Windows keeps using Wi-Fi/Ethernet for the Internet instead of
/// the tablet's mobile data (the cable remains usable for the Internet when it is the only link).
/// Blocks until the elevated helper finishes (or the user declines).
pub fn allow_link(link: &UsbLink) -> anyhow::Result<()> {
    anyhow::ensure!(!link.simulated, "simulated links need no firewall change");
    let exe = std::env::current_exe()?
        .to_string_lossy()
        .replace('\'', "''");
    let alias = link.alias.replace('\'', "''");
    let rule = format!("VisionPro-USB-In-{}", link.interface_index);
    let script = format!(
        "$ErrorActionPreference = 'SilentlyContinue'; \
         Set-NetConnectionProfile -InterfaceIndex {index} -NetworkCategory Private; \
         Set-NetIPInterface -InterfaceIndex {index} -AddressFamily IPv4 -AutomaticMetric Disabled -InterfaceMetric 4000; \
         Remove-NetFirewallRule -Name '{rule}'; \
         New-NetFirewallRule -Name '{rule}' -DisplayName 'Vision-Pro (USB)' -Description 'Liaison USB directe avec une tablette' \
           -Direction Inbound -Action Allow -Program '{exe}' -Profile Private,Public -InterfaceAlias '{alias}' | Out-Null",
        index = link.interface_index,
    );
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, utf16);
    let parameters = wide(&format!(
        "-NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -EncodedCommand {encoded}"
    ));
    let verb = wide("runas");
    let file = wide("powershell.exe");
    unsafe {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NO_CONSOLE,
            hwnd: HWND::default(),
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(parameters.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        ShellExecuteExW(&mut info)
            .map_err(|_| anyhow::anyhow!("L’autorisation Windows a été refusée."))?;
        if info.hProcess.is_invalid() {
            return Ok(());
        }
        let waited = WaitForSingleObject(info.hProcess, 60_000);
        let mut code = 0u32;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        anyhow::ensure!(
            waited == WAIT_OBJECT_0,
            "Windows n’a pas terminé la configuration à temps."
        );
        anyhow::ensure!(
            code == 0,
            "La configuration réseau Windows a échoué (code {code})."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firewall_port_lists() {
        assert!(port_listed("47800", 47800));
        assert!(port_listed("80, 443,47800", 47800));
        assert!(port_listed("47000-48000", 47800));
        assert!(port_listed("*", 47800));
        assert!(!port_listed("47801", 47800));
        assert!(!port_listed("RPC", 47800));
    }

    #[test]
    fn program_paths_are_compared_normalized() {
        std::env::set_var("VP_TEST_ROOT", "C:\\Users\\Léa\\AppData\\Local");
        assert_eq!(
            normalize_path("%VP_TEST_ROOT%\\Vision-Pro\\vision-pro.exe"),
            normalize_path("c:/users/léa/appdata/local/vision-pro/Vision-Pro.EXE")
        );
    }

    #[test]
    fn guid_format_matches_adapter_names() {
        let guid = GUID::from_u128(0x2a3b4c5d_0001_0203_0405_060708090a0b);
        assert_eq!(guid_string(&guid), "{2A3B4C5D-0001-0203-0405-060708090A0B}");
    }

    /// Runs the real scanners on this PC. Never fails on a machine without tablets; it guards
    /// against crashes and false positives from ordinary USB devices (storage, cameras, hubs).
    #[test]
    fn scanners_run_on_this_pc() {
        let entries = usb_entries();
        let tablets = scan_devices();
        let links = scan_links();
        for tablet in &tablets {
            assert!(entries
                .iter()
                .any(|e| e.vendor_id == tablet.vendor_id && e.product_id == tablet.product_id));
        }
        for link in &links {
            assert!(link.local_ip.is_private());
        }
        let _com = ComApartment::enter();
        let _ = network_categories();
        let mut diagnostics = Diagnostics::default();
        let _ = diagnostics.firewall(NetworkCategory::Public, 47800);
        eprintln!("internet interface: {:?}", internet_interface());
        eprintln!(
            "usb nodes: {} · tablets: {tablets:?} · links: {links:?}",
            entries.len()
        );
    }

    /// Diagnostic against this PC's real rules (prints only: rules differ between machines).
    #[test]
    fn firewall_diagnosis_on_this_pc() {
        let _com = ComApartment::enter();
        let exe =
            std::env::var("VP_FIREWALL_EXE").unwrap_or_else(|_| r"C:\Windows\notepad.exe".into());
        for (name, profile) in [
            ("public", NET_FW_PROFILE2_PUBLIC.0),
            ("private", NET_FW_PROFILE2_PRIVATE.0),
        ] {
            eprintln!(
                "firewall {name} for {exe}: {:?}",
                firewall_state_for(&exe, profile, 47800)
            );
        }
    }
}
