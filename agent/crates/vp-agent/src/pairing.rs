//! Pairing state: the "Connect Device" window (one-time QR token + 6-digit code) and the single
//! pending approval shown on the Mac. Discovery never implies authorization: every new device
//! requires an explicit "Allow" on the Mac, and code/QR proofs are verified in constant time.

use crate::crypto;
use crate::events::PairingRequestInfo;
use crate::util::{from_b64u, now_ms, random_code, random_token};
use parking_lot::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

pub const CONNECT_WINDOW_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_CODE_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone)]
pub struct ConnectWindow {
    pub token: String,
    pub code: String,
    pub opened: Instant,
    pub expires_at_ms: u64,
    code_attempts: u32,
}

impl ConnectWindow {
    fn fresh() -> Self {
        Self {
            token: random_token(18),
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
    }

    pub fn window(&self) -> Option<ConnectWindow> {
        let mut guard = self.window.lock();
        if guard.as_ref().is_some_and(|w| w.expired()) {
            *guard = None;
        }
        guard.clone()
    }

    /// Verifies a QR-token or code proof bound to this handshake transcript.
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
        let Some(mac) = from_b64u(mac_b64u, Some(32)) else {
            return Err(ProofError::Invalid);
        };
        match kind {
            "qr" => {
                let expected = crypto::pair_proof_mac(&window.token, transcript);
                if crypto::ct_eq(&mac, &expected) {
                    Ok("qr")
                } else {
                    Err(ProofError::Invalid)
                }
            }
            "code" => {
                if window.code_attempts >= MAX_CODE_ATTEMPTS {
                    return Err(ProofError::CodeExhausted);
                }
                window.code_attempts += 1;
                let expected = crypto::pair_proof_mac(&window.code, transcript);
                if crypto::ct_eq(&mac, &expected) {
                    Ok("code")
                } else {
                    if window.code_attempts >= MAX_CODE_ATTEMPTS {
                        // Burn the code; the Mac shows a new one.
                        window.code = random_code();
                        window.code_attempts = 0;
                        return Err(ProofError::CodeExhausted);
                    }
                    Err(ProofError::Invalid)
                }
            }
            _ => Err(ProofError::Invalid),
        }
    }

    /// Registers the single pending approval. Returns `None` if another request is already waiting.
    pub fn begin(&self, info: PairingRequestInfo) -> Option<oneshot::Receiver<bool>> {
        let mut guard = self.pending.lock();
        if guard.as_ref().is_some_and(|p| {
            p.info.expires_at > now_ms() && p.responder.as_ref().is_some_and(|r| !r.is_closed())
        }) {
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

    /// Consumes the QR token after a successful pairing so a photographed QR cannot be replayed.
    pub fn consume_token(&self) {
        if let Some(window) = self.window.lock().as_mut() {
            window.token = random_token(18);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let qr = crate::util::b64u(&crypto::pair_proof_mac(&window.token, &transcript));
        assert_eq!(manager.verify_proof("qr", &qr, &transcript), Ok("qr"));
    }
}
