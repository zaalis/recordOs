//! Registry of active sessions. Each session is owned by its signaling task; the registry only keeps
//! a command channel (to end it instantly) and a shared, UI-facing snapshot.

use crate::protocol::{AgentStats, AppliedVideo};
use crate::store::Permission;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizationEpoch {
    global: u64,
    device: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandshakeEpoch {
    global: u64,
    sequence: u64,
}

#[derive(Default)]
struct DeviceEpoch {
    generation: u64,
    changed_at: u64,
}

#[derive(Default)]
struct Epochs {
    global: u64,
    sequence: u64,
    devices: HashMap<String, DeviceEpoch>,
}

impl Epochs {
    fn end_device(&mut self, device_id: &str) {
        self.sequence = self.sequence.wrapping_add(1);
        let sequence = self.sequence;
        let device = self.devices.entry(device_id.to_string()).or_default();
        device.generation = device.generation.wrapping_add(1);
        device.changed_at = sequence;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EndReason {
    User,
    Revoked,
    Expired,
    Shutdown,
    Replaced,
    Error,
    Stopped,
    /// The cable carrying the session was unplugged.
    LinkLost,
}

impl EndReason {
    pub fn as_str(self) -> &'static str {
        match self {
            EndReason::User => "user",
            EndReason::Revoked => "revoked",
            EndReason::Expired => "expired",
            EndReason::Shutdown => "shutdown",
            EndReason::Replaced => "replaced",
            EndReason::Error => "error",
            EndReason::Stopped => "stopped",
            EndReason::LinkLost => "link",
        }
    }
}

/// Physical path of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Link {
    /// Wi-Fi or Ethernet on the local network.
    Lan,
    /// A USB cable between the tablet and this PC.
    Usb,
    /// The Internet relay.
    Relay,
}

#[derive(Debug)]
pub enum SessionCommand {
    SetPermission(Permission),
    /// A cable path to this PC appeared; the iPad may move its session to it.
    UsbAvailable(Vec<String>),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub device_id: String,
    pub device_name: String,
    pub device_kind: String,
    pub ip: String,
    pub started_at: u64,
    pub permission: Permission,
    pub streaming: bool,
    pub video: Option<AppliedVideo>,
    pub stats: Option<AgentStats>,
    pub rtt_ms: Option<f32>,
    pub active_app: Option<String>,
    pub via_relay: bool,
    pub link: Link,
    /// Address of this PC the session arrived on.
    pub local_ip: Option<String>,
}

pub struct SessionEntry {
    pub info: Arc<RwLock<SessionInfo>>,
    pub commands: mpsc::Sender<SessionCommand>,
    /// Terminal requests must not compete with best-effort control updates in `commands`.
    pub end: watch::Sender<Option<EndReason>>,
}

#[derive(Default)]
pub struct SessionRegistry {
    sessions: RwLock<HashMap<String, SessionEntry>>,
    epochs: Mutex<Epochs>,
}

impl SessionRegistry {
    /// Registers a session, ending any previous session of the same device.
    pub fn register(
        &self,
        info: SessionInfo,
        commands: mpsc::Sender<SessionCommand>,
        end: watch::Sender<Option<EndReason>>,
        authorization_epoch: AuthorizationEpoch,
    ) -> Option<Arc<RwLock<SessionInfo>>> {
        // Serialize final session admission against disconnect, revoke, and disconnect-all.
        let mut epochs = self.epochs.lock();
        if epochs.global != authorization_epoch.global
            || epochs
                .devices
                .get(&info.device_id)
                .map_or(0, |epoch| epoch.generation)
                != authorization_epoch.device
        {
            return None;
        }
        epochs.end_device(&info.device_id);
        let shared = Arc::new(RwLock::new(info.clone()));
        let mut map = self.sessions.write();
        for entry in map.values() {
            if entry.info.read().device_id == info.device_id {
                entry.end.send_replace(Some(EndReason::Replaced));
            }
        }
        map.insert(
            info.id.clone(),
            SessionEntry {
                info: shared.clone(),
                commands,
                end,
            },
        );
        Some(shared)
    }

    pub fn unregister(&self, id: &str) {
        self.sessions.write().remove(id);
    }

    pub fn snapshot(&self) -> Vec<SessionInfo> {
        let mut list: Vec<SessionInfo> = self
            .sessions
            .read()
            .values()
            .map(|e| e.info.read().clone())
            .collect();
        list.sort_by_key(|s| s.started_at);
        list
    }

    pub fn count(&self) -> usize {
        self.sessions.read().len()
    }

    pub fn end_session(&self, id: &str, reason: EndReason) -> bool {
        let mut epochs = self.epochs.lock();
        let sessions = self.sessions.read();
        sessions.get(id).is_some_and(|e| {
            let device_id = e.info.read().device_id.clone();
            epochs.end_device(&device_id);
            e.end.send_replace(Some(reason));
            true
        })
    }

    pub fn end_device(&self, device_id: &str, reason: EndReason) -> usize {
        self.end_device_with(device_id, reason, || ()).1
    }

    /// Serialize an authorization-store mutation with device disconnect/revoke.
    /// The operation runs while the generation lock is held, so a stale pairing
    /// approval cannot re-create trust after a concurrent revoke.
    pub fn end_device_with<T>(
        &self,
        device_id: &str,
        reason: EndReason,
        operation: impl FnOnce() -> T,
    ) -> (T, usize) {
        let mut epochs = self.epochs.lock();
        epochs.end_device(device_id);
        let result = operation();
        let map = self.sessions.read();
        let count = map
            .values()
            .filter(|e| e.info.read().device_id == device_id)
            .filter(|e| {
                e.end.send_replace(Some(reason));
                true
            })
            .count();
        (result, count)
    }

    /// Run an authorization write only if no relevant disconnect/revoke raced
    /// the handshake. Lock ordering is epochs -> device store throughout.
    pub fn with_current_authorization<T>(
        &self,
        device_id: &str,
        authorization_epoch: AuthorizationEpoch,
        operation: impl FnOnce() -> T,
    ) -> Option<T> {
        let epochs = self.epochs.lock();
        let current_device = epochs.devices.get(device_id);
        if epochs.global != authorization_epoch.global
            || current_device.map_or(0, |epoch| epoch.generation) != authorization_epoch.device
        {
            return None;
        }
        Some(operation())
    }

    pub fn end_all(&self, reason: EndReason) -> usize {
        let mut epochs = self.epochs.lock();
        epochs.global = epochs.global.wrapping_add(1);
        let map = self.sessions.read();
        map.values()
            .filter(|e| {
                e.end.send_replace(Some(reason));
                true
            })
            .count()
    }

    /// Ends the sessions carried by a local address (a cable that was unplugged).
    pub fn end_by_local_ip(&self, local_ip: &str, reason: EndReason) -> usize {
        let map = self.sessions.read();
        map.values()
            .filter(|e| e.info.read().local_ip.as_deref() == Some(local_ip))
            .filter(|e| {
                e.end.send_replace(Some(reason));
                true
            })
            .count()
    }

    /// Tells every local-network session that the PC is also reachable through a cable.
    pub fn offer_usb(&self, urls: &[String]) -> usize {
        if urls.is_empty() {
            return 0;
        }
        let map = self.sessions.read();
        map.values()
            .filter(|e| e.info.read().link == Link::Lan)
            .filter(|e| {
                e.commands
                    .try_send(SessionCommand::UsbAvailable(urls.to_vec()))
                    .is_ok()
            })
            .count()
    }

    pub fn set_permission(&self, device_id: &str, permission: Permission) {
        for entry in self.sessions.read().values() {
            if entry.info.read().device_id == device_id {
                let _ = entry
                    .commands
                    .try_send(SessionCommand::SetPermission(permission));
            }
        }
    }

    pub fn is_device_online(&self, device_id: &str) -> bool {
        self.sessions
            .read()
            .values()
            .any(|e| e.info.read().device_id == device_id)
    }

    pub fn handshake_epoch(&self) -> HandshakeEpoch {
        let epochs = self.epochs.lock();
        HandshakeEpoch {
            global: epochs.global,
            sequence: epochs.sequence,
        }
    }

    pub fn authorization_epoch(
        &self,
        device_id: &str,
        handshake: HandshakeEpoch,
    ) -> Option<AuthorizationEpoch> {
        let epochs = self.epochs.lock();
        let device = epochs.devices.get(device_id);
        if epochs.global != handshake.global
            || device.is_some_and(|epoch| epoch.changed_at > handshake.sequence)
        {
            return None;
        }
        Some(AuthorizationEpoch {
            global: handshake.global,
            device: device.map_or(0, |epoch| epoch.generation),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_info(id: &str, device_id: &str) -> SessionInfo {
        SessionInfo {
            id: id.to_string(),
            device_id: device_id.to_string(),
            device_name: "iPad".to_string(),
            device_kind: "tablet".to_string(),
            ip: "192.168.1.5".to_string(),
            started_at: 1,
            permission: Permission::View,
            streaming: false,
            video: None,
            stats: None,
            rtt_ms: None,
            active_app: None,
            via_relay: false,
            link: Link::Lan,
            local_ip: Some("192.168.1.2".to_string()),
        }
    }

    #[test]
    fn terminal_signal_is_not_lost_when_the_command_queue_is_full() {
        let registry = SessionRegistry::default();
        let (commands, _command_rx) = mpsc::channel(1);
        commands
            .try_send(SessionCommand::SetPermission(Permission::Control))
            .unwrap();
        let (end, end_rx) = watch::channel(None);
        let epoch = registry
            .authorization_epoch("device-1", registry.handshake_epoch())
            .unwrap();
        registry
            .register(session_info("session-1", "device-1"), commands, end, epoch)
            .unwrap();

        assert_eq!(registry.end_all(EndReason::User), 1);
        assert_eq!(*end_rx.borrow(), Some(EndReason::User));
    }

    #[test]
    fn replacing_a_device_session_signals_the_old_session() {
        let registry = SessionRegistry::default();
        let (old_commands, _old_rx) = mpsc::channel(1);
        let (old_end, old_end_rx) = watch::channel(None);
        let epoch = registry
            .authorization_epoch("device-1", registry.handshake_epoch())
            .unwrap();
        registry
            .register(
                session_info("old", "device-1"),
                old_commands,
                old_end,
                epoch,
            )
            .unwrap();
        let epoch = registry
            .authorization_epoch("device-1", registry.handshake_epoch())
            .unwrap();
        let (new_commands, _new_rx) = mpsc::channel(1);
        let (new_end, _new_end_rx) = watch::channel(None);

        registry
            .register(
                session_info("new", "device-1"),
                new_commands,
                new_end,
                epoch,
            )
            .unwrap();

        assert_eq!(*old_end_rx.borrow(), Some(EndReason::Replaced));
        assert_eq!(registry.count(), 2);
    }

    #[test]
    fn disconnect_prevents_an_in_flight_handshake_from_registering() {
        let registry = SessionRegistry::default();
        let handshake = registry.handshake_epoch();
        let epoch = registry.authorization_epoch("device-1", handshake).unwrap();
        registry.end_device("device-1", EndReason::User);
        assert!(registry
            .authorization_epoch("device-1", handshake)
            .is_none());
        let (commands, _command_rx) = mpsc::channel(1);
        let (end, _end_rx) = watch::channel(None);

        assert!(registry
            .register(session_info("session-1", "device-1"), commands, end, epoch)
            .is_none());
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn disconnect_all_prevents_every_in_flight_handshake_from_registering() {
        let registry = SessionRegistry::default();
        let handshake = registry.handshake_epoch();
        let epoch = registry.authorization_epoch("device-1", handshake).unwrap();
        registry.end_all(EndReason::User);
        assert!(registry
            .authorization_epoch("device-1", handshake)
            .is_none());
        let (commands, _command_rx) = mpsc::channel(1);
        let (end, _end_rx) = watch::channel(None);

        assert!(registry
            .register(session_info("session-1", "device-1"), commands, end, epoch)
            .is_none());
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn authorization_write_is_rejected_after_device_disconnect() {
        let registry = SessionRegistry::default();
        let handshake = registry.handshake_epoch();
        let epoch = registry.authorization_epoch("device-1", handshake).unwrap();
        registry.end_device("device-1", EndReason::User);

        let mut wrote_trust = false;
        assert!(registry
            .with_current_authorization("device-1", epoch, || wrote_trust = true)
            .is_none());
        assert!(!wrote_trust);
    }

    #[test]
    fn disconnecting_one_device_does_not_cancel_another_devices_handshake() {
        let registry = SessionRegistry::default();
        let handshake = registry.handshake_epoch();
        registry.end_device("device-a", EndReason::User);
        let epoch = registry
            .authorization_epoch("device-b", handshake)
            .expect("device A disconnect must not invalidate device B");
        let (commands, _command_rx) = mpsc::channel(1);
        let (end, _end_rx) = watch::channel(None);

        assert!(registry
            .register(session_info("session-b", "device-b"), commands, end, epoch)
            .is_some());
    }

    #[test]
    fn ending_one_session_does_not_cancel_another_devices_handshake() {
        let registry = SessionRegistry::default();
        let (commands_a, _commands_a_rx) = mpsc::channel(1);
        let (end_a, _end_a_rx) = watch::channel(None);
        let epoch_a = registry
            .authorization_epoch("device-a", registry.handshake_epoch())
            .unwrap();
        registry
            .register(
                session_info("session-a", "device-a"),
                commands_a,
                end_a,
                epoch_a,
            )
            .unwrap();
        let handshake_b = registry.handshake_epoch();

        assert!(registry.end_session("session-a", EndReason::User));
        let epoch_b = registry
            .authorization_epoch("device-b", handshake_b)
            .expect("ending device A session must not invalidate device B");
        let (commands_b, _commands_b_rx) = mpsc::channel(1);
        let (end_b, _end_b_rx) = watch::channel(None);

        assert!(registry
            .register(
                session_info("session-b", "device-b"),
                commands_b,
                end_b,
                epoch_b
            )
            .is_some());
    }
}
