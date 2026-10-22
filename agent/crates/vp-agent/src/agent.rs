//! Agent facade used by the desktop UI: service lifecycle, pairing, devices, sessions, settings.

use crate::config::Settings;
use crate::control::ControlEngine;
use crate::discovery::{Discovery, NearbyAgent};
use crate::events::{AgentEvent, PairingRequestInfo};
use crate::identity::AgentIdentity;
use crate::pairing::PairingManager;
use crate::protocol::AgentCaps;
use crate::relay::{RelayHub, RelayStatus};
use crate::security::{AuditEntry, AuditLog, Guard, Severity};
use crate::server::{self, ServerHandle};
use crate::session::{EndReason, SessionInfo, SessionRegistry};
use crate::store::{DeviceRecord, DeviceStore, Permission};
use crate::tls::LocalAuthority;
use crate::util::now_ms;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Semaphore};

/// Shared state reachable from servers and signaling tasks.
pub struct Core {
    pub settings: RwLock<Settings>,
    pub identity: RwLock<Arc<AgentIdentity>>,
    pub devices: RwLock<DeviceStore>,
    pub audit: AuditLog,
    pub guard: Guard,
    pub pairing: PairingManager,
    pub sessions: SessionRegistry,
    pub events: broadcast::Sender<AgentEvent>,
    pub control: Arc<dyn ControlEngine>,
    pub hostname: String,
    pub handshake_slots: Arc<Semaphore>,
    pub authority: RwLock<Option<Arc<LocalAuthority>>>,
    pub tls: RwLock<Option<Arc<rustls::ServerConfig>>>,
    pub cert_ips: RwLock<Vec<Ipv4Addr>>,
    pub nearby: Arc<RwLock<Vec<NearbyAgent>>>,
    pub relay: Arc<RelayHub>,
    /// Running mDNS announcement, updated when this PC's addresses change.
    pub discovery: RwLock<Option<Arc<Discovery>>>,
    /// Tablets and cable links currently detected (USB Direct).
    pub usb: RwLock<crate::usb::UsbStatus>,
    pub usb_wake: Mutex<Option<crossbeam_channel::Sender<crate::usb::Wake>>>,
    pub usb_simulation: RwLock<Option<crate::usb::Simulation>>,
    /// Set after a firewall change so the next scan does not reuse a cached diagnosis.
    pub usb_diagnostics_stale: std::sync::atomic::AtomicBool,
    network_refresh: Mutex<()>,
}

struct Service {
    // Dropped first: the monitor must stop before discovery and the servers go away.
    _usb: crate::usb::Monitor,
    _server: ServerHandle,
    _discovery: Option<Arc<Discovery>>,
    maintenance: tokio::task::JoinHandle<()>,
    relay: Option<tokio::sync::watch::Sender<bool>>,
    started_at: u64,
}

impl Drop for Service {
    fn drop(&mut self) {
        self.maintenance.abort();
        if let Some(relay) = &self.relay {
            let _ = relay.send(true);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusSnapshot {
    pub running: bool,
    pub error: Option<String>,
    pub started_at: Option<u64>,
    pub computer_name: String,
    pub agent_id: String,
    pub fingerprint: String,
    pub full_fingerprint: String,
    pub hostname: String,
    pub port: u16,
    pub addresses: Vec<String>,
    pub network_mode: crate::config::NetworkMode,
    pub relay: RelayStatus,
    pub sessions: Vec<SessionInfo>,
    pub pending_pairing: Option<PairingRequestInfo>,
    pub connect_window_open: bool,
    pub caps: AgentCaps,
    pub locked_sources: usize,
    pub store_integrity_warning: bool,
    pub version: String,
    pub usb: crate::usb::UsbStatus,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConnectNetwork {
    /// Same Wi-Fi or Ethernet network as this PC.
    Lan,
    /// The private Tailscale network: works from anywhere once the iPad is on it.
    Tailscale,
}

/// An address to type on the iPad. It opens the setup page, which installs this PC's trust
/// profile the first time and then continues to the Record OS app.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectAddress {
    pub network: ConnectNetwork,
    pub url: String,
    /// The app itself, for an iPad that already trusts this PC.
    pub app_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectInfo {
    pub code: String,
    pub expires_at: u64,
    pub computer_name: String,
    pub fingerprint: String,
    pub addresses: Vec<ConnectAddress>,
    pub authority_fingerprint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    #[serde(flatten)]
    pub record: DeviceRecord,
    pub online: bool,
    pub fingerprint_display: String,
}

pub struct Agent {
    core: Arc<Core>,
    runtime: tokio::runtime::Runtime,
    service: Mutex<Option<Service>>,
    bootstrap: Mutex<Option<(ServerHandle, u16)>>,
    last_error: Mutex<Option<String>>,
}

impl Agent {
    pub fn new(control: Arc<dyn ControlEngine>) -> anyhow::Result<Agent> {
        crate::paths::ensure_data_dir()?;
        let settings = Settings::load();
        let identity = Arc::new(AgentIdentity::load_or_create()?);
        let devices = DeviceStore::load(&identity);
        let audit = AuditLog::new();
        if devices.integrity_warning {
            audit.record(
                Severity::Critical,
                "security.store-integrity",
                "The device list failed its integrity check. All devices now require confirmation.",
            );
        }
        let (events, _) = broadcast::channel(256);
        let guard = Guard::new(
            settings.security.lockout_threshold,
            settings.security.lockout_minutes,
        );
        let hostname = crate::net::mdns_hostname(identity.public_raw())
            .trim_end_matches('.')
            .to_string();

        let core = Arc::new(Core {
            settings: RwLock::new(settings),
            identity: RwLock::new(identity),
            devices: RwLock::new(devices),
            audit,
            guard,
            pairing: PairingManager::default(),
            sessions: SessionRegistry::default(),
            events,
            control,
            hostname,
            handshake_slots: Arc::new(Semaphore::new(16)),
            authority: RwLock::new(None),
            tls: RwLock::new(None),
            cert_ips: RwLock::new(vec![]),
            nearby: Arc::new(RwLock::new(vec![])),
            relay: Arc::new(RelayHub::default()),
            discovery: RwLock::new(None),
            usb: RwLock::new(crate::usb::UsbStatus::default()),
            usb_wake: Mutex::new(None),
            usb_simulation: RwLock::new(None),
            usb_diagnostics_stale: std::sync::atomic::AtomicBool::new(false),
            network_refresh: Mutex::new(()),
        });

        // Forward audit entries to UI observers.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("vp-agent")
            .enable_all()
            .build()?;
        {
            let core = core.clone();
            let mut audit_rx = core.audit.subscribe();
            runtime.spawn(async move {
                while let Ok(entry) = audit_rx.recv().await {
                    let _ = core.events.send(AgentEvent::Audit(entry));
                }
            });
        }

        Ok(Agent {
            core,
            runtime,
            service: Mutex::new(None),
            bootstrap: Mutex::new(None),
            last_error: Mutex::new(None),
        })
    }

    pub fn core(&self) -> &Arc<Core> {
        &self.core
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.core.events.subscribe()
    }

    pub fn is_running(&self) -> bool {
        self.service.lock().is_some()
    }

    pub(crate) fn refresh_certificate(core: &Core) -> anyhow::Result<()> {
        let name = core.settings.read().computer_name.clone();
        let authority = match core.authority.read().clone() {
            Some(a) => a,
            None => Arc::new(LocalAuthority::load_or_create(&name)?),
        };
        let ips = certificate_addresses();
        let hostnames = vec![
            core.hostname.clone(),
            format!("{}.local", crate::net::system_hostname().to_lowercase()),
        ];
        let certificate = authority.issue_server_certificate(&hostnames, &ips)?;
        *core.tls.write() = Some(certificate.rustls_config(&authority)?);
        *core.cert_ips.write() = ips;
        *core.authority.write() = Some(authority);
        Ok(())
    }

    pub fn start_service(&self) -> anyhow::Result<()> {
        let mut service = self.service.lock();
        if service.is_some() {
            return Ok(());
        }
        let core = self.core.clone();
        let result: anyhow::Result<Service> = self.runtime.block_on(async {
            Self::refresh_certificate(&core)?;
            let server = server::start(core.clone()).await?;
            let settings = core.settings.read().clone();
            let discovery = if settings.network.advertise_mdns {
                let identity = core.identity.read().clone();
                Discovery::start(
                    &identity.agent_id,
                    &settings.computer_name,
                    &core.hostname,
                    &lan_addresses(),
                    settings.network.https_port,
                    core.nearby.clone(),
                )
                .map_err(|e| tracing::warn!(error = %e, "mDNS unavailable"))
                .ok()
                .map(Arc::new)
            } else {
                None
            };
            *core.discovery.write() = discovery.clone();
            // Detects tablets on the USB bus and keeps addresses fresh (Wi-Fi changes, cables).
            let usb = crate::usb::Monitor::start(core.clone());
            let maintenance = tokio::spawn(maintenance_loop(core.clone()));
            let relay = match (&settings.network.mode, &settings.network.relay_url) {
                (crate::config::NetworkMode::RemoteAccess, Some(url)) => {
                    let (stop, stopped) = tokio::sync::watch::channel(false);
                    tokio::spawn(crate::relay::run(
                        core.clone(),
                        url.clone(),
                        settings.network.relay_token.clone(),
                        stopped,
                    ));
                    Some(stop)
                }
                _ => None,
            };
            Ok(Service {
                _usb: usb,
                _server: server,
                _discovery: discovery,
                maintenance,
                relay,
                started_at: now_ms(),
            })
        });
        match result {
            Ok(started) => {
                *service = Some(started);
                *self.last_error.lock() = None;
                self.core.audit.record(
                    Severity::Info,
                    "service.started",
                    "Record OS service started.",
                );
                let _ = self.core.events.send(AgentEvent::ServiceChanged {
                    running: true,
                    error: None,
                });
                Ok(())
            }
            Err(error) => {
                *self.last_error.lock() = Some(error.to_string());
                self.core.audit.record(
                    Severity::Warning,
                    "service.error",
                    format!("Service could not start: {error}"),
                );
                let _ = self.core.events.send(AgentEvent::ServiceChanged {
                    running: false,
                    error: Some(error.to_string()),
                });
                Err(error)
            }
        }
    }

    pub fn stop_service(&self) {
        self.core.sessions.end_all(EndReason::Stopped);
        self.close_connect_window();
        let stopped = {
            let _guard = self.runtime.enter();
            let service = self.service.lock().take();
            *self.core.discovery.write() = None;
            let was_running = service.is_some();
            drop(service);
            was_running
        };
        if stopped {
            self.core.audit.record(
                Severity::Info,
                "service.stopped",
                "Record OS service stopped.",
            );
            let _ = self.core.events.send(AgentEvent::ServiceChanged {
                running: false,
                error: None,
            });
        }
    }

    pub fn status(&self) -> StatusSnapshot {
        let settings = self.core.settings.read().clone();
        let identity = self.core.identity.read().clone();
        let service = self.service.lock();
        let port = settings.network.https_port;
        let mut addresses = vec![format!("https://{}:{port}", self.core.hostname)];
        addresses.extend(
            self.core
                .cert_ips
                .read()
                .iter()
                .map(|ip| format!("https://{ip}:{port}")),
        );
        StatusSnapshot {
            running: service.is_some(),
            error: self.last_error.lock().clone(),
            started_at: service.as_ref().map(|s| s.started_at),
            computer_name: settings.computer_name.clone(),
            agent_id: identity.agent_id.clone(),
            fingerprint: crate::crypto::format_fingerprint(identity.public_raw()),
            full_fingerprint: crate::crypto::fingerprint_of(identity.public_raw()),
            hostname: self.core.hostname.clone(),
            port,
            addresses,
            network_mode: settings.network.mode,
            relay: self.core.relay.status(),
            sessions: self.core.sessions.snapshot(),
            pending_pairing: self.core.pairing.pending(),
            connect_window_open: self.core.pairing.window().is_some(),
            caps: self.core.control.capabilities(),
            locked_sources: self.core.guard.locked_sources().len(),
            store_integrity_warning: self.core.devices.read().integrity_warning,
            version: crate::VERSION.to_string(),
            usb: self.core.usb.read().clone(),
        }
    }

    // --- USB Direct ---------------------------------------------------------------------

    /// Test hook: behaves as if a tablet were connected by cable on `ip` (a loopback address).
    /// The developer loopback endpoint also listens on that address.
    pub fn simulate_usb(&self, ip: Ipv4Addr) {
        *self.core.usb_simulation.write() = Some(crate::usb::Simulation {
            ip,
            connected: true,
        });
        crate::usb::wake(&self.core);
    }

    /// Plugs or unplugs the simulated cable. Returns the new state.
    pub fn toggle_simulated_usb(&self) -> Option<bool> {
        let connected = {
            let mut simulation = self.core.usb_simulation.write();
            let simulation = simulation.as_mut()?;
            simulation.connected = !simulation.connected;
            simulation.connected
        };
        crate::usb::wake(&self.core);
        Some(connected)
    }

    /// Makes the cable network private and allows Record OS on it (asks for UAC consent).
    pub fn allow_usb_link(&self, id: &str) -> anyhow::Result<()> {
        let link = self
            .core
            .usb
            .read()
            .links
            .iter()
            .find(|l| l.id == id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Cette liaison USB n’est plus active."))?;
        crate::platform::usb::allow_link(&link)?;
        self.core.audit.record(
            Severity::Notice,
            "usb.network-allowed",
            format!(
                "La liaison USB « {} » est désormais un réseau privé autorisé pour Record OS.",
                link.alias
            ),
        );
        self.core
            .usb_diagnostics_stale
            .store(true, std::sync::atomic::Ordering::SeqCst);
        crate::usb::wake(&self.core);
        Ok(())
    }

    // --- Pairing ----------------------------------------------------------------

    /// Opens "Connect Device" (or returns what it shows). Observers are told only when the window
    /// opens or gets a new code, so the UI can call this again on every agent event.
    pub fn open_connect_window(&self, refresh: bool) -> anyhow::Result<ConnectInfo> {
        anyhow::ensure!(
            self.is_running(),
            "Start Record OS before connecting a device."
        );
        let previous = self.core.pairing.window().map(|w| w.code);
        let window = if refresh {
            self.core.pairing.refresh_window()
        } else {
            self.core.pairing.open_window()
        };
        let settings = self.core.settings.read().clone();
        let identity = self.core.identity.read().clone();
        let url = settings
            .network
            .relay_url
            .as_deref()
            .unwrap_or("wss://vision-pro.34-73-186-88.sslip.io")
            .replacen("wss://", "https://", 1);
        let addresses = vec![ConnectAddress {
            network: ConnectNetwork::Lan,
            url: url.clone(),
            app_url: url,
        }];
        if previous.as_deref() != Some(window.code.as_str()) {
            let _ = self.core.events.send(AgentEvent::ConnectWindowChanged);
        }
        Ok(ConnectInfo {
            code: window.code,
            expires_at: window.expires_at_ms,
            computer_name: settings.computer_name,
            fingerprint: crate::crypto::format_fingerprint(identity.public_raw()),
            addresses,
            authority_fingerprint: self
                .core
                .authority
                .read()
                .as_ref()
                .map(|a| a.fingerprint_sha256())
                .unwrap_or_default(),
        })
    }

    /// The code "Connect Device" shows right now, if its window is open.
    pub fn connect_code(&self) -> Option<String> {
        self.core.pairing.window().map(|window| window.code)
    }

    pub fn close_connect_window(&self) {
        self.core.pairing.close_window();
        let handle = {
            let _guard = self.runtime.enter();
            self.bootstrap.lock().take().map(|(handle, _)| handle)
        };
        drop(handle);
        let _ = self.core.events.send(AgentEvent::ConnectWindowChanged);
    }

    pub fn respond_pairing(&self, id: &str, approve: bool) -> bool {
        self.core.pairing.respond(id, approve)
    }

    // --- Devices ----------------------------------------------------------------

    pub fn devices(&self) -> Vec<DeviceView> {
        let devices = self.core.devices.read();
        let mut list: Vec<DeviceView> = devices
            .list()
            .iter()
            .map(|record| DeviceView {
                online: self.core.sessions.is_device_online(&record.id),
                fingerprint_display: crate::util::from_b64u(&record.public_key, Some(65))
                    .map(|raw| crate::crypto::format_fingerprint(&raw))
                    .unwrap_or_default(),
                record: record.clone(),
            })
            .collect();
        list.sort_by(|a, b| b.record.last_seen_at.cmp(&a.record.last_seen_at));
        list
    }

    fn mutate_devices(
        &self,
        f: impl FnOnce(&mut DeviceStore) -> Option<(String, String)>,
        kind: &str,
        message: impl Fn(&str) -> String,
    ) -> bool {
        let identity = self.core.identity.read().clone();
        let changed = {
            let mut devices = self.core.devices.write();
            let result = f(&mut devices);
            if result.is_some() {
                if let Err(error) = devices.save(&identity) {
                    tracing::error!(%error, "failed to save devices");
                }
            }
            result
        };
        match changed {
            Some((id, name)) => {
                self.core
                    .audit
                    .record(Severity::Notice, kind, message(&name))
                    .device(&id, &name);
                let _ = self.core.events.send(AgentEvent::DevicesChanged);
                true
            }
            None => false,
        }
    }

    fn mutate_devices_and_end_device(
        &self,
        id: &str,
        reason: EndReason,
        f: impl FnOnce(&mut DeviceStore) -> Option<(String, String)>,
        kind: &str,
        message: impl Fn(&str) -> String,
    ) -> bool {
        let identity = self.core.identity.read().clone();
        let sessions = &self.core.sessions;
        let pairing = &self.core.pairing;
        let device_store = &self.core.devices;
        let (changed, _) = sessions.end_device_with(id, reason, || {
            pairing.cancel_pending_device(id);
            let mut devices = device_store.write();
            let result = f(&mut devices);
            if result.is_some() {
                if let Err(error) = devices.save(&identity) {
                    tracing::error!(%error, "failed to save devices");
                }
            }
            result
        });
        match changed {
            Some((device_id, name)) => {
                self.core
                    .audit
                    .record(Severity::Notice, kind, message(&name))
                    .device(&device_id, &name);
                let _ = self.core.events.send(AgentEvent::DevicesChanged);
                true
            }
            None => false,
        }
    }

    fn device_name(&self, id: &str) -> Option<String> {
        self.core.devices.read().get(id).map(|d| d.name.clone())
    }

    pub fn rename_device(&self, id: &str, name: &str) -> bool {
        let old = self.device_name(id);
        self.mutate_devices(
            |d| {
                d.rename(id, name).then(|| {
                    (
                        id.to_string(),
                        d.get(id).map(|r| r.name.clone()).unwrap_or_default(),
                    )
                })
            },
            "device.renamed",
            |new| format!("{} renamed to {new}.", old.clone().unwrap_or_default()),
        )
    }

    pub fn trust_device(&self, id: &str) -> bool {
        let name = self.device_name(id).unwrap_or_default();
        self.mutate_devices(
            |d| d.set_trusted(id).then(|| (id.to_string(), name.clone())),
            "device.trusted",
            |n| format!("{n} was marked as trusted."),
        )
    }

    pub fn set_device_permission(&self, id: &str, permission: Permission) -> bool {
        let name = self.device_name(id).unwrap_or_default();
        let ok = self.mutate_devices(
            |d| {
                d.set_permission(id, permission)
                    .then(|| (id.to_string(), name.clone()))
            },
            "device.permission",
            |n| {
                format!(
                    "{n} can now {}.",
                    if permission == Permission::Control {
                        "control this PC"
                    } else {
                        "only view this PC"
                    }
                )
            },
        );
        if ok {
            self.core.sessions.set_permission(id, permission);
        }
        ok
    }

    /// Revocation ends live sessions and completely removes the authorization.
    pub fn revoke_device(&self, id: &str) -> bool {
        let name = self.device_name(id).unwrap_or_default();
        let revoked = self.mutate_devices_and_end_device(
            id,
            EndReason::Revoked,
            |d| d.revoke(id).then(|| (id.to_string(), name.clone())),
            "device.removed",
            |n| format!("{n} was removed. A new pairing approval is required to reconnect."),
        );
        revoked
    }

    pub fn forget_device(&self, id: &str) -> bool {
        let name = self.device_name(id).unwrap_or_default();
        let forgotten = self.mutate_devices_and_end_device(
            id,
            EndReason::Revoked,
            |d| d.forget(id).then(|| (id.to_string(), name.clone())),
            "device.forgotten",
            |n| format!("{n} was removed."),
        );
        forgotten
    }

    pub fn disconnect_device(&self, id: &str) -> usize {
        let sessions = &self.core.sessions;
        let pairing = &self.core.pairing;
        let (_, count) = sessions.end_device_with(id, EndReason::User, || {
            pairing.cancel_pending_device(id);
        });
        if count > 0 {
            let name = self.device_name(id).unwrap_or_default();
            self.core
                .audit
                .record(
                    Severity::Info,
                    "session.disconnected",
                    format!("{name} was disconnected from this PC."),
                )
                .device(id, &name);
        }
        count
    }

    pub fn end_session(&self, session_id: &str) -> bool {
        self.core.sessions.end_session(session_id, EndReason::User)
    }

    /// Emergency stop: ends every session and closes the pairing window.
    pub fn disconnect_all(&self) -> usize {
        self.core.pairing.close_window();
        let count = self.core.sessions.end_all(EndReason::User);
        if count > 0 {
            self.core.audit.record(
                Severity::Notice,
                "session.disconnect-all",
                format!("All sessions were ended ({count})."),
            );
        }
        count
    }

    // --- Settings & security ------------------------------------------------------

    pub fn settings(&self) -> Settings {
        self.core.settings.read().clone()
    }

    pub fn update_settings(&self, mut next: Settings) -> anyhow::Result<()> {
        next.sanitize();
        let previous = self.core.settings.read().clone();
        next.save()?;
        *self.core.settings.write() = next.clone();
        self.core.guard.configure(
            next.security.lockout_threshold,
            next.security.lockout_minutes,
        );
        self.core
            .audit
            .record(Severity::Info, "settings.changed", "Settings were updated.");
        let _ = self.core.events.send(AgentEvent::SettingsChanged);
        if previous.network.usb_direct != next.network.usb_direct {
            crate::usb::wake(&self.core);
        }

        let restart = previous.network.https_port != next.network.https_port
            || previous.network.mode != next.network.mode
            || previous.network.advertise_mdns != next.network.advertise_mdns
            || previous.network.relay_url != next.network.relay_url
            || previous.network.relay_token != next.network.relay_token
            || previous.advanced.developer_loopback != next.advanced.developer_loopback
            || previous.computer_name != next.computer_name;
        if restart && self.is_running() {
            self.stop_service();
            self.start_service()?;
        }
        Ok(())
    }

    pub fn audit(&self, limit: usize) -> Vec<AuditEntry> {
        self.core.audit.recent(limit.min(500))
    }

    pub fn unlock_sources(&self) {
        self.core.guard.unlock_all();
        self.core.audit.record(
            Severity::Notice,
            "security.unlocked",
            "All blocked sources were unblocked.",
        );
    }

    /// Creates a new agent identity. Every paired device must pair again.
    pub fn reset_identity(&self) -> anyhow::Result<()> {
        let was_running = self.is_running();
        self.stop_service();
        let identity = Arc::new(AgentIdentity::reset()?);
        {
            let mut devices = self.core.devices.write();
            devices.clear();
            devices.save(&identity)?;
        }
        *self.core.identity.write() = identity;
        self.core.audit.record(
            Severity::Critical,
            "security.identity-reset",
            "This PC's identity was reset. All paired devices were removed.",
        );
        let _ = self.core.events.send(AgentEvent::DevicesChanged);
        if was_running {
            self.start_service()?;
        }
        Ok(())
    }

    pub fn shutdown(&self) {
        self.stop_service();
    }
}

async fn maintenance_loop(core: Arc<Core>) {
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    loop {
        tick.tick().await;
        let identity = core.identity.read().clone();
        let days = core.settings.read().security.trust_expiry_days;
        let expired = {
            let mut devices = core.devices.write();
            let expired = devices.expire_stale(days);
            if !expired.is_empty() {
                let _ = devices.save(&identity);
            }
            expired
        };
        if !expired.is_empty() {
            core.audit.record(
                Severity::Notice,
                "device.trust-expired",
                format!(
                    "{} inactive device(s) now require confirmation.",
                    expired.len()
                ),
            );
            let _ = core.events.send(AgentEvent::DevicesChanged);
        }
        // Fallback for missed address notifications (the USB monitor normally reacts first).
        refresh_network(&core);
    }
}

/// Keeps the TLS certificate and the mDNS announcement in step with this PC's addresses, so an
/// iPad can reach the PC right after a DHCP renewal, a Wi-Fi switch or a cable being plugged in.
pub(crate) fn refresh_network(core: &Core) {
    let _serialized = core.network_refresh.lock();
    let ips = certificate_addresses();
    if *core.cert_ips.read() == ips {
        return;
    }
    if let Err(error) = Agent::refresh_certificate(core) {
        tracing::warn!(%error, "certificate refresh failed");
        return;
    }
    if let Some(discovery) = core.discovery.read().clone() {
        discovery.set_addresses(&lan_addresses());
    }
    tracing::info!(addresses = ?ips, "local addresses changed");
    let _ = core.events.send(AgentEvent::NetworkChanged);
}

fn lan_addresses() -> Vec<Ipv4Addr> {
    crate::net::lan_interfaces()
        .into_iter()
        .map(|i| i.ip)
        .collect()
}

/// Addresses the iPad may use to reach this PC: the local network, then Tailscale.
fn certificate_addresses() -> Vec<Ipv4Addr> {
    let mut ips = lan_addresses();
    ips.extend(crate::net::overlay_addresses());
    ips
}
