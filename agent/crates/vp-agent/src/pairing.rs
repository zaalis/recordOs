//! Pairing state: the "Connect Device" window (a 6-digit code shown on the PC) and the single
//! pending approval shown on the PC for devices that must be confirmed again.
//!
//! Discovery never implies authorization: a new device must prove it knows the code currently
//! displayed on this PC. The proof is a MAC bound to the handshake transcript, verified in
//! constant time; a code allows five attempts and is replaced after each successful pairing.

use crate::crypto;
use crate::events::PairingRequestInfo;
use crate::util::{from_b64u, now_ms, random_code};
use parking_lot::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

pub const CONNECT_WINDOW_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_CODE_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone)]
pub struct ConnectWindow {
    pub code: String,
    pub opened: Instant,
    pub expires_at_ms: u64,
    code_attempts: u32,
}

impl ConnectWindow {
    fn fresh() -> Self {
        Self {
            code: random_code(),
            opened: Instant::now(),
            expires_at_ms: now_ms() + CONNECT_WINDOW_TTL.as_millis() as u64,
            code_attempts: 0,
        }
    }
    fn expired(&self) -> bool {
        self.opened.elapsed() > CONNECT_WINDOW_TTL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofError {
    NoWindow,
    Invalid,
    CodeExhausted,
}

struct Pending {
    info: PairingRequestInfo,
    responder: Option<oneshot::Sender<bool>>,
}

#[derive(Default)]
pub struct PairingManager {
    window: Mutex<Option<ConnectWindow>>,
    pending: Mutex<Option<Pending>>,
}

impl PairingManager {
    pub fn open_window(&self) -> ConnectWindow {
        let mut guard = self.window.lock();
        match guard.as_ref() {
            Some(window) if !window.expired() => window.clone(),
            _ => {
                let window = ConnectWindow::fresh();
                *guard = Some(window.clone());
                window
            }
        }
    }

    pub fn refresh_window(&self) -> ConnectWindow {
        let window = ConnectWindow::fresh();
        *self.window.lock() = Some(window.clone());
        window
    }

    pub fn close_window(&self) {
        *self.window.lock() = None;
        // Closing the pairing UI also invalidates any approval already shown there. Otherwise a
        // response arriving after service shutdown could authorize a handshake that was in flight.
        *self.pending.lock() = None;
    }

    /// Cancels an approval for one device without changing the open pairing window.
    pub fn cancel_pending_device(&self, device_id: &str) {
        let mut pending = self.pending.lock();
        if pending
            .as_ref()
            .is_some_and(|request| request.info.device_id == device_id)
        {
            *pending = None;
        }
    }

    pub fn window(&self) -> Option<ConnectWindow> {
        let mut guard = self.window.lock();
        if guard.as_ref().is_some_and(|w| w.expired()) {
            *guard = None;
        }
        guard.clone()
    }

    /// Verifies a code proof bound to this handshake transcript.
    pub fn verify_proof(
        &self,
        kind: &str,
        mac_b64u: &str,
        transcript: &[u8],
    ) -> Result<&'static str, ProofError> {
        let mut guard = self.window.lock();
        let Some(window) = guard.as_mut().filter(|w| !w.expired()) else {
            return Err(ProofError::NoWindow);
        };
        let Some(mac) = from_b64u(mac_b64u, Some(32)).filter(|_| kind == "code") else {
            return Err(ProofError::Invalid);
        };
        if window.code_attempts >= MAX_CODE_ATTEMPTS {
            return Err(ProofError::CodeExhausted);
        }
        window.code_attempts += 1;
        let expected = crypto::pair_proof_mac(&window.code, transcript);
        if crypto::ct_eq(&mac, &expected) {
            return Ok("code");
        }
        if window.code_attempts >= MAX_CODE_ATTEMPTS {
            // Burn the code; the PC shows a new one.
            window.code = random_code();
            window.code_attempts = 0;
            return Err(ProofError::CodeExhausted);
        }
        Err(ProofError::Invalid)
    }

    /// Registers the single pending approval. A reconnect from the same device
    /// replaces its earlier request, which commonly happens when mobile Safari
    /// reloads the page before the old WebSocket close reaches the PC.
    /// Requests from a different device remain serialized.
    pub fn begin(&self, info: PairingRequestInfo) -> Option<oneshot::Receiver<bool>> {
        let mut guard = self.pending.lock();
        let active_other_device = guard.as_ref().is_some_and(|p| {
            p.info.expires_at > now_ms()
                && p.responder.as_ref().is_some_and(|r| !r.is_closed())
                && p.info.device_id != info.device_id
        });
        if active_other_device {
            return None;
        }
        let (tx, rx) = oneshot::channel();
        *guard = Some(Pending {
            info,
            responder: Some(tx),
        });
        Some(rx)
    }

    pub fn respond(&self, id: &str, approve: bool) -> bool {
        let mut guard = self.pending.lock();
        match guard.as_mut() {
            Some(p) if p.info.id == id => {
                let delivered = p
                    .responder
                    .take()
                    .is_some_and(|tx| tx.send(approve).is_ok());
                *guard = None;
                delivered
            }
            _ => false,
        }
    }

    pub fn clear(&self, id: &str) {
        let mut guard = self.pending.lock();
        if guard.as_ref().is_some_and(|p| p.info.id == id) {
            *guard = None;
        }
    }

    pub fn pending(&self) -> Option<PairingRequestInfo> {
        self.pending
            .lock()
            .as_ref()
            .filter(|p| p.info.expires_at > now_ms())
            .map(|p| p.info.clone())
    }

    /// Replaces the code after a successful pairing: a code seen over someone's shoulder cannot
    /// pair a second device. The PC shows the new one.
    pub fn consume_code(&self) {
        if let Some(window) = self.window.lock().as_mut() {
            window.code = random_code();
            window.code_attempts = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str, device_id: &str) -> PairingRequestInfo {
        PairingRequestInfo {
            id: id.to_string(),
            device_id: device_id.to_string(),
            device_name: "iPad".to_string(),
            device_kind: "tablet".to_string(),
            ip: "192.168.1.5".to_string(),
            sas: "123456".to_string(),
            method: "reconfirm".to_string(),
            created_at: now_ms(),
            expires_at: now_ms() + 60_000,
        }
    }

    #[test]
    fn code_attempts_are_limited() {
        let manager = PairingManager::default();
        let window = manager.open_window();
        let transcript = [1u8; 32];
        let wrong = crate::util::b64u(&crypto::pair_proof_mac("000000x", &transcript));
        for _ in 0..4 {
            assert_eq!(
                manager.verify_proof("code", &wrong, &transcript),
                Err(ProofError::Invalid)
            );
        }
        assert_eq!(
            manager.verify_proof("code", &wrong, &transcript),
            Err(ProofError::CodeExhausted)
        );
        let old = crate::util::b64u(&crypto::pair_proof_mac(&window.code, &transcript));
        assert!(manager.verify_proof("code", &old, &transcript).is_err());
        let fresh = manager.window().unwrap();
        let good = crate::util::b64u(&crypto::pair_proof_mac(&fresh.code, &transcript));
        assert_eq!(
            manager.verify_proof("qr", &good, &transcript),
            Err(ProofError::Invalid),
            "only code proofs exist"
        );
        assert_eq!(manager.verify_proof("code", &good, &transcript), Ok("code"));
    }

    #[test]
    fn a_used_code_is_replaced() {
        let manager = PairingManager::default();
        let window = manager.open_window();
        let transcript = [2u8; 32];
        let proof = crate::util::b64u(&crypto::pair_proof_mac(&window.code, &transcript));
        assert_eq!(
            manager.verify_proof("code", &proof, &transcript),
            Ok("code")
        );
        manager.consume_code();
        assert_ne!(manager.window().unwrap().code, window.code);
        assert!(manager.verify_proof("code", &proof, &transcript).is_err());
    }

    #[test]
    fn retry_from_same_device_replaces_pending_request() {
        let manager = PairingManager::default();
        let mut first = manager.begin(request("first", "ipad-1")).unwrap();
        let _second = manager.begin(request("second", "ipad-1")).unwrap();
        assert!(matches!(
            first.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        assert!(manager.begin(request("third", "ipad-2")).is_none());
    }

    #[test]
    fn closing_window_cancels_an_already_visible_approval() {
        let manager = PairingManager::default();
        manager.open_window();
        let mut decision = manager.begin(request("pending", "ipad-1")).unwrap();

        manager.close_window();

        assert!(manager.pending().is_none());
        assert!(matches!(
            decision.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
    }

    #[test]
    fn disconnecting_one_device_cancels_only_its_pending_approval() {
        let manager = PairingManager::default();
        manager.open_window();
        let mut decision = manager.begin(request("pending", "ipad-1")).unwrap();

        manager.cancel_pending_device("ipad-2");
        assert!(manager.pending().is_some());
        assert!(matches!(
            decision.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));

        manager.cancel_pending_device("ipad-1");
        assert!(manager.pending().is_none());
        assert!(matches!(
            decision.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        assert!(manager.window().is_some());
    }
}
