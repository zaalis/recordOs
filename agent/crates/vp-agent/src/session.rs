//! Registry of active sessions. Each session is owned by its signaling task; the registry only keeps
//! a command channel (to end it instantly) and a shared, UI-facing snapshot.

use crate::protocol::{AgentStats, AppliedVideo};
use crate::store::Permission;
use parking_lot::RwLock;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

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
    /// A USB cable between the tablet and this Mac.
    Usb,
    /// The Internet relay.
    Relay,
}

#[derive(Debug)]
pub enum SessionCommand {
    End(EndReason),
    SetPermission(Permission),
    /// A cable path to this Mac appeared; the iPad may move its session to it.
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
    /// Address of this Mac the session arrived on.
    pub local_ip: Option<String>,
}

pub struct SessionEntry {
    pub info: Arc<RwLock<SessionInfo>>,
    pub commands: mpsc::Sender<SessionCommand>,
}

#[derive(Default)]
pub struct SessionRegistry {
    sessions: RwLock<HashMap<String, SessionEntry>>,
}

impl SessionRegistry {
    /// Registers a session, ending any previous session of the same device.
    pub fn register(
        &self,
        info: SessionInfo,
        commands: mpsc::Sender<SessionCommand>,
    ) -> Arc<RwLock<SessionInfo>> {
        let shared = Arc::new(RwLock::new(info.clone()));
        let mut map = self.sessions.write();
        for entry in map.values() {
            if entry.info.read().device_id == info.device_id {
                let _ = entry
                    .commands
                    .try_send(SessionCommand::End(EndReason::Replaced));
            }
        }
        map.insert(
            info.id.clone(),
            SessionEntry {
                info: shared.clone(),
                commands,
            },
        );
        shared
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
        self.sessions
            .read()
            .get(id)
            .is_some_and(|e| e.commands.try_send(SessionCommand::End(reason)).is_ok())
    }

    pub fn end_device(&self, device_id: &str, reason: EndReason) -> usize {
        let map = self.sessions.read();
        map.values()
            .filter(|e| e.info.read().device_id == device_id)
            .filter(|e| e.commands.try_send(SessionCommand::End(reason)).is_ok())
            .count()
    }

    pub fn end_all(&self, reason: EndReason) -> usize {
        let map = self.sessions.read();
        map.values()
            .filter(|e| e.commands.try_send(SessionCommand::End(reason)).is_ok())
            .count()
    }

    /// Ends the sessions carried by a local address (a cable that was unplugged).
    pub fn end_by_local_ip(&self, local_ip: &str, reason: EndReason) -> usize {
        let map = self.sessions.read();
        map.values()
            .filter(|e| e.info.read().local_ip.as_deref() == Some(local_ip))
            .filter(|e| e.commands.try_send(SessionCommand::End(reason)).is_ok())
            .count()
    }

    /// Tells every local-network session that the Mac is also reachable through a cable.
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
}
