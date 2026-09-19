//! Authorized device registry (%APPDATA%\Vision-Pro\devices.json), integrity-protected with an
//! HMAC keyed by the DPAPI-protected identity. A tampered file downgrades every device to untrusted.

use crate::identity::AgentIdentity;
use crate::paths;
use crate::util::{b64u, from_b64u, now_ms};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeviceStatus {
    Trusted,
    /// Known key, but trust expired or was withdrawn: the next connection needs confirmation on the PC.
    Untrusted,
    /// Legacy on-disk status. Revoked records are removed during loading.
    ///
    /// Kept solely so installations produced before the deletion-based revocation
    /// policy can be migrated without making their device store unreadable.
    Revoked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    Control,
    View,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub public_key: String,
    pub fingerprint: String,
    pub paired_at: u64,
    pub last_seen_at: Option<u64>,
    pub last_ip: Option<String>,
    pub status: DeviceStatus,
    pub permission: Permission,
    pub pairing_method: String,
    pub revoked_at: Option<u64>,
    pub key_rotated_at: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    payload: String,
    mac: String,
}

pub struct DeviceStore {
    devices: Vec<DeviceRecord>,
    pub integrity_warning: bool,
}

impl DeviceStore {
    fn path() -> std::path::PathBuf {
        paths::data_dir().join("devices.json")
    }

    pub fn load(identity: &AgentIdentity) -> DeviceStore {
        let Ok(bytes) = std::fs::read(Self::path()) else {
            return DeviceStore {
                devices: vec![],
                integrity_warning: false,
            };
        };
        let parsed: Option<(StoreFile, Vec<DeviceRecord>)> =
            serde_json::from_slice::<StoreFile>(&bytes)
                .ok()
                .and_then(|file| {
                    serde_json::from_str::<Vec<DeviceRecord>>(&file.payload)
                        .ok()
                        .map(|d| (file, d))
                });
        match parsed {
            Some((file, mut devices)) => {
                let expected = identity.integrity_mac(file.payload.as_bytes());
                let valid = from_b64u(&file.mac, Some(32))
                    .is_some_and(|mac| crate::crypto::ct_eq(&mac, &expected));
                if !valid {
                    tracing::error!(
                        "device store integrity check failed; all devices downgraded to untrusted"
                    );
                    for device in &mut devices {
                        if device.status == DeviceStatus::Trusted {
                            device.status = DeviceStatus::Untrusted;
                        }
                    }
                }
                // Older releases retained revoked entries for history. A revoked
                // device must now be completely removed so the same iPad can pair
                // again with its existing local identity.
                let removed_revoked = devices.len();
                devices.retain(|device| device.status != DeviceStatus::Revoked);
                let removed_revoked = removed_revoked != devices.len();
                let store = DeviceStore {
                    devices,
                    integrity_warning: !valid,
                };
                if removed_revoked {
                    if let Err(error) = store.save(identity) {
                        tracing::error!(%error, "failed to persist revoked-device cleanup");
                    }
                }
                store
            }
            None => {
                tracing::error!("device store unreadable; starting empty");
                let _ = std::fs::rename(
                    Self::path(),
                    Self::path().with_extension(format!("unreadable-{}.json", now_ms())),
                );
                DeviceStore {
                    devices: vec![],
                    integrity_warning: true,
                }
            }
        }
    }

    pub fn save(&self, identity: &AgentIdentity) -> anyhow::Result<()> {
        paths::ensure_data_dir()?;
        let payload = serde_json::to_string(&self.devices)?;
        let mac = b64u(&identity.integrity_mac(payload.as_bytes()));
        let file = StoreFile {
            version: 1,
            payload,
            mac,
        };
        paths::write_atomic(&Self::path(), &serde_json::to_vec_pretty(&file)?)?;
        Ok(())
    }

    pub fn list(&self) -> &[DeviceRecord] {
        &self.devices
    }

    pub fn get(&self, id: &str) -> Option<&DeviceRecord> {
        self.devices.iter().find(|d| d.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut DeviceRecord> {
        self.devices.iter_mut().find(|d| d.id == id)
    }

    pub fn insert_paired(
        &mut self,
        id: &str,
        name: &str,
        kind: &str,
        public_key: &[u8],
        method: &str,
        ip: &str,
    ) {
        let now = now_ms();
        let record = DeviceRecord {
            id: id.to_string(),
            name: name.to_string(),
            kind: kind.to_string(),
            public_key: b64u(public_key),
            fingerprint: crate::crypto::fingerprint_of(public_key),
            paired_at: now,
            last_seen_at: Some(now),
            last_ip: Some(ip.to_string()),
            status: DeviceStatus::Trusted,
            permission: Permission::Control,
            pairing_method: method.to_string(),
            revoked_at: None,
            key_rotated_at: None,
        };
        self.devices.retain(|d| d.id != id);
        self.devices.push(record);
    }

    pub fn touch(&mut self, id: &str, ip: &str) {
        if let Some(device) = self.get_mut(id) {
            device.last_seen_at = Some(now_ms());
            device.last_ip = Some(ip.to_string());
        }
    }

    pub fn rename(&mut self, id: &str, name: &str) -> bool {
        match self.get_mut(id) {
            Some(device) => {
                device.name = crate::util::sanitize_name(name, 48);
                true
            }
            None => false,
        }
    }

    pub fn set_trusted(&mut self, id: &str) -> bool {
        match self.get_mut(id) {
            Some(device) if device.status == DeviceStatus::Untrusted => {
                device.status = DeviceStatus::Trusted;
                device.last_seen_at = Some(now_ms());
                true
            }
            _ => false,
        }
    }

    pub fn set_permission(&mut self, id: &str, permission: Permission) -> bool {
        match self.get_mut(id) {
            Some(device) if device.status != DeviceStatus::Revoked => {
                device.permission = permission;
                true
            }
            _ => false,
        }
    }

    /// Removes every authorization record for a device. Future access must use a
    /// fresh pairing request approved on this PC.
    pub fn revoke(&mut self, id: &str) -> bool {
        self.forget(id)
    }

    pub fn forget(&mut self, id: &str) -> bool {
        let before = self.devices.len();
        self.devices.retain(|d| d.id != id);
        before != self.devices.len()
    }

    /// Removes every stored device, for example when this PC receives a new
    /// identity. No stale authorization is retained under the new identity.
    pub fn clear(&mut self) -> bool {
        if self.devices.is_empty() {
            false
        } else {
            self.devices.clear();
            true
        }
    }

    /// Key rotation keeps the record (name, permissions, history) under the new key-derived id.
    pub fn rotate_key(&mut self, id: &str, new_public: &[u8]) -> Option<String> {
        let new_id = crate::crypto::id_from_public_key(new_public);
        if self.get(&new_id).is_some() {
            return None;
        }
        let device = self.get_mut(id)?;
        device.id = new_id.clone();
        device.public_key = b64u(new_public);
        device.fingerprint = crate::crypto::fingerprint_of(new_public);
        device.key_rotated_at = Some(now_ms());
        Some(new_id)
    }

    /// Downgrades trusted devices that have not connected within `days`. Returns affected ids.
    pub fn expire_stale(&mut self, days: u32) -> Vec<String> {
        let cutoff = now_ms().saturating_sub(days as u64 * 86_400_000);
        let mut expired = vec![];
        for device in &mut self.devices {
            let last = device.last_seen_at.unwrap_or(device.paired_at);
            if device.status == DeviceStatus::Trusted && last < cutoff {
                device.status = DeviceStatus::Untrusted;
                expired.push(device.id.clone());
            }
        }
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str) -> DeviceRecord {
        DeviceRecord {
            id: id.to_string(),
            name: "iPad".to_string(),
            kind: "ipad".to_string(),
            public_key: "key".to_string(),
            fingerprint: "fingerprint".to_string(),
            paired_at: 1,
            last_seen_at: Some(1),
            last_ip: Some("192.168.1.2".to_string()),
            status: DeviceStatus::Trusted,
            permission: Permission::Control,
            pairing_method: "qr".to_string(),
            revoked_at: None,
            key_rotated_at: None,
        }
    }

    #[test]
    fn revocation_removes_the_device_record() {
        let mut store = DeviceStore {
            devices: vec![device("ipad-a"), device("ipad-b")],
            integrity_warning: false,
        };

        assert!(store.revoke("ipad-a"));
        assert!(store.get("ipad-a").is_none());
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].id, "ipad-b");
    }

    #[test]
    fn clear_removes_every_authorization() {
        let mut store = DeviceStore {
            devices: vec![device("ipad-a")],
            integrity_warning: false,
        };

        assert!(store.clear());
        assert!(store.list().is_empty());
        assert!(!store.clear());
    }
}
