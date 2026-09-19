//! Security log: append-only JSON lines on disk (rotated) plus an in-memory ring for the UI.

use crate::paths;
use crate::util::now_ms;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::VecDeque;
use std::io::Write;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Notice,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub ts: u64,
    pub severity: Severity,
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
}

pub struct AuditLog {
    ring: Mutex<VecDeque<AuditEntry>>,
    file: Mutex<Option<std::fs::File>>,
    tx: broadcast::Sender<AuditEntry>,
}

const RING_SIZE: usize = 500;
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

impl AuditLog {
    pub fn new() -> AuditLog {
        let (tx, _) = broadcast::channel(256);
        let log = AuditLog {
            ring: Mutex::new(VecDeque::with_capacity(RING_SIZE)),
            file: Mutex::new(None),
            tx,
        };
        log.open_file();
        log.load_tail();
        log
    }

    fn path() -> std::path::PathBuf {
        paths::data_dir().join("logs").join("security.log")
    }

    fn open_file(&self) {
        let _ = paths::ensure_data_dir();
        let path = Self::path();
        if std::fs::metadata(&path)
            .map(|m| m.len() > MAX_FILE_BYTES)
            .unwrap_or(false)
        {
            let _ = std::fs::rename(&path, path.with_extension("1.log"));
        }
        *self.file.lock() = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok();
    }

    fn load_tail(&self) {
        let Ok(text) = std::fs::read_to_string(Self::path()) else {
            return;
        };
        let mut ring = self.ring.lock();
        for line in text
            .lines()
            .rev()
            .take(RING_SIZE)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                let entry = AuditEntry {
                    ts: value["ts"].as_u64().unwrap_or(0),
                    severity: match value["severity"].as_str() {
                        Some("critical") => Severity::Critical,
                        Some("warning") => Severity::Warning,
                        Some("notice") => Severity::Notice,
                        _ => Severity::Info,
                    },
                    kind: value["kind"].as_str().unwrap_or("").to_string(),
                    message: value["message"].as_str().unwrap_or("").to_string(),
                    ip: value["ip"].as_str().map(str::to_string),
                    device_id: value["deviceId"].as_str().map(str::to_string),
                    device_name: value["deviceName"].as_str().map(str::to_string),
                };
                ring.push_back(entry);
            }
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AuditEntry> {
        self.tx.subscribe()
    }

    pub fn recent(&self, limit: usize) -> Vec<AuditEntry> {
        self.ring.lock().iter().rev().take(limit).cloned().collect()
    }

    pub fn record(
        &self,
        severity: Severity,
        kind: &str,
        message: impl Into<String>,
    ) -> AuditBuilder<'_> {
        AuditBuilder {
            log: self,
            entry: Some(AuditEntry {
                ts: now_ms(),
                severity,
                kind: kind.to_string(),
                message: message.into(),
                ip: None,
                device_id: None,
                device_name: None,
            }),
        }
    }

    fn commit(&self, entry: AuditEntry) {
        match entry.severity {
            Severity::Critical | Severity::Warning => {
                tracing::warn!(kind = %entry.kind, ip = ?entry.ip, "{}", entry.message)
            }
            _ => tracing::info!(kind = %entry.kind, ip = ?entry.ip, "{}", entry.message),
        }
        if let Some(file) = self.file.lock().as_mut() {
            if let Ok(line) = serde_json::to_string(&entry) {
                let _ = writeln!(file, "{line}");
            }
        }
        {
            let mut ring = self.ring.lock();
            if ring.len() == RING_SIZE {
                ring.pop_front();
            }
            ring.push_back(entry.clone());
        }
        let _ = self.tx.send(entry);
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder that commits the entry when dropped, so call sites stay one expression.
pub struct AuditBuilder<'a> {
    log: &'a AuditLog,
    entry: Option<AuditEntry>,
}

impl AuditBuilder<'_> {
    pub fn ip(mut self, ip: impl ToString) -> Self {
        if let Some(e) = self.entry.as_mut() {
            e.ip = Some(ip.to_string());
        }
        self
    }
    pub fn device(mut self, id: &str, name: &str) -> Self {
        if let Some(e) = self.entry.as_mut() {
            e.device_id = Some(id.to_string());
            e.device_name = Some(name.to_string());
        }
        self
    }
}

impl Drop for AuditBuilder<'_> {
    fn drop(&mut self) {
        if let Some(entry) = self.entry.take() {
            self.log.commit(entry);
        }
    }
}
