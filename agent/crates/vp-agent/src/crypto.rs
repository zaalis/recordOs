//! Handshake & secure channel — exact mirror of packages/protocol/src/crypto.ts.
//! Any change here must be reflected there (and bumps the protocol version).

use crate::util::{b64u, lp};
use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::Aes256Gcm;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

pub const LABEL_TRANSCRIPT: &str = "Vision-Pro/1 transcript";
pub const LABEL_COMMIT: &str = "Vision-Pro/1 commit";
pub const LABEL_AGENT_SIG: &str = "Vision-Pro/1 agent";
pub const LABEL_DEVICE_SIG: &str = "Vision-Pro/1 device";
pub const LABEL_C2S: &str = "Vision-Pro/1 c2s";
pub const LABEL_S2C: &str = "Vision-Pro/1 s2c";
pub const LABEL_SAS: &str = "Vision-Pro/1 sas";
pub const LABEL_SESSION_ID: &str = "Vision-Pro/1 session-id";
pub const LABEL_FRAME: &str = "Vision-Pro/1 frame";
pub const LABEL_REKEY: &str = "Vision-Pro/1 rekey";
pub const LABEL_PAIR_SECRET: &str = "Vision-Pro/1 pair-secret";
pub const LABEL_ROTATE: &str = "Vision-Pro/1 rotate";

pub const DIR_C2S: u32 = 1;
pub const DIR_S2C: u32 = 2;

pub const REKEY_AFTER_MESSAGES: u64 = 50_000;
pub const REKEY_AFTER: std::time::Duration = std::time::Duration::from_secs(600);

pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

/// base64url(SHA-256(pub)[0..16]) — 22 characters.
pub fn id_from_public_key(public_raw: &[u8]) -> String {
    b64u(&sha256(&[public_raw])[..16])
}

pub fn fingerprint_of(public_raw: &[u8]) -> String {
    b64u(&sha256(&[public_raw]))
}

/// "ABCD EF01 ..." — 8 groups of the first 16 bytes, for display next to QR codes.
pub fn format_fingerprint(public_raw: &[u8]) -> String {
    let digest = sha256(&[public_raw]);
    let hex: String = digest[..16].iter().map(|b| format!("{b:02X}")).collect();
    hex.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn public_raw(key: &VerifyingKey) -> [u8; 65] {
    let point = key.to_encoded_point(false);
    let mut out = [0u8; 65];
    out.copy_from_slice(point.as_bytes());
    out
}

pub fn parse_public_key(raw: &[u8]) -> Option<VerifyingKey> {
    if raw.len() != 65 || raw[0] != 0x04 {
        return None;
    }
    VerifyingKey::from_sec1_bytes(raw).ok()
}

pub fn sign(key: &SigningKey, label: &str, data: &[u8]) -> [u8; 64] {
    let mut message = Vec::with_capacity(label.len() + data.len());
    message.extend_from_slice(label.as_bytes());
    message.extend_from_slice(data);
    let signature: Signature = key.sign(&message);
    let mut out = [0u8; 64];
    out.copy_from_slice(&signature.to_bytes());
    out
}

/// Verifies a P1363 (r||s) ECDSA-P256-SHA256 signature. Accepts high-S signatures produced by WebCrypto.
pub fn verify(key: &VerifyingKey, label: &str, data: &[u8], signature: &[u8]) -> bool {
    let Ok(signature) = Signature::from_slice(signature) else {
        return false;
    };
    let signature = signature.normalize_s().unwrap_or(signature);
    let mut message = Vec::with_capacity(label.len() + data.len());
    message.extend_from_slice(label.as_bytes());
    message.extend_from_slice(data);
    key.verify(&message, &signature).is_ok()
}

pub fn commit(server_eph: &[u8], server_nonce: &[u8]) -> [u8; 32] {
    let mut buf = Vec::new();
    buf.extend_from_slice(LABEL_COMMIT.as_bytes());
    lp(&mut buf, server_eph);
    lp(&mut buf, server_nonce);
    sha256(&[&buf])
}

pub struct TranscriptInput<'a> {
    pub agent_id: &'a str,
    pub agent_pub: &'a [u8],
    pub commit: &'a [u8],
    pub mode: &'a str,
    pub client_eph: &'a [u8],
    pub client_nonce: &'a [u8],
    pub device_id: &'a str,
    pub device_pub: &'a [u8],
    pub device_name: &'a str,
    pub device_kind: &'a str,
    pub server_eph: &'a [u8],
    pub server_nonce: &'a [u8],
}

pub fn transcript(t: &TranscriptInput) -> [u8; 32] {
    let mut buf = Vec::with_capacity(512);
    buf.extend_from_slice(LABEL_TRANSCRIPT.as_bytes());
    lp(&mut buf, t.agent_id.as_bytes());
    lp(&mut buf, t.agent_pub);
    lp(&mut buf, t.commit);
    lp(&mut buf, t.mode.as_bytes());
    lp(&mut buf, t.client_eph);
    lp(&mut buf, t.client_nonce);
    lp(&mut buf, t.device_id.as_bytes());
    lp(&mut buf, t.device_pub);
    lp(&mut buf, t.device_name.as_bytes());
    lp(&mut buf, t.device_kind.as_bytes());
    lp(&mut buf, t.server_eph);
    lp(&mut buf, t.server_nonce);
    sha256(&[&buf])
}

fn hkdf_expand(ikm: &[u8], salt: &[u8], info: &str, out: &mut [u8]) {
    Hkdf::<Sha256>::new(Some(salt), ikm)
        .expand(info.as_bytes(), out)
        .expect("hkdf output length is small");
}

pub struct SessionKeys {
    pub c2s: [u8; 32],
    pub s2c: [u8; 32],
    pub sas: String,
    pub session_id: String,
    pub transcript: [u8; 32],
}

impl Drop for SessionKeys {
    fn drop(&mut self) {
        self.c2s.zeroize();
        self.s2c.zeroize();
    }
}

pub fn derive_session_keys(shared_secret: &[u8], transcript: [u8; 32]) -> SessionKeys {
    let mut c2s = [0u8; 32];
    let mut s2c = [0u8; 32];
    let mut sas = [0u8; 8];
    let mut sid = [0u8; 16];
    hkdf_expand(shared_secret, &transcript, LABEL_C2S, &mut c2s);
    hkdf_expand(shared_secret, &transcript, LABEL_S2C, &mut s2c);
    hkdf_expand(shared_secret, &transcript, LABEL_SAS, &mut sas);
    hkdf_expand(shared_secret, &transcript, LABEL_SESSION_ID, &mut sid);
    SessionKeys {
        c2s,
        s2c,
        sas: sas_from_bytes(sas),
        session_id: b64u(&sid),
        transcript,
    }
}

pub fn sas_from_bytes(bytes: [u8; 8]) -> String {
    format!("{:06}", u64::from_be_bytes(bytes) % 1_000_000)
}

pub fn next_epoch_key(key: &[u8; 32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    hkdf_expand(key, &[], LABEL_REKEY, &mut out);
    out
}

pub fn pair_proof_mac(secret: &str, transcript: &[u8]) -> [u8; 32] {
    let key = sha256(&[LABEL_PAIR_SECRET.as_bytes(), secret.as_bytes()]);
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&key).expect("hmac key");
    mac.update(transcript);
    mac.finalize().into_bytes().into()
}

pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

pub fn rotation_message(session_id: &str, new_pub: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    lp(&mut buf, session_id.as_bytes());
    lp(&mut buf, new_pub);
    buf
}

// ---------------------------------------------------------------------------
// Secure channel
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ChannelError {
    #[error("frame out of sequence")]
    Sequence,
    #[error("authentication failed")]
    Auth,
    #[error("malformed frame")]
    Malformed,
}

struct Directional {
    key: [u8; 32],
    cipher: Aes256Gcm,
    direction: u32,
    epoch: u32,
    seq: u64,
    started: std::time::Instant,
}

impl Directional {
    fn new(key: [u8; 32], direction: u32) -> Self {
        Self {
            cipher: Aes256Gcm::new(&key.into()),
            key,
            direction,
            epoch: 0,
            seq: 0,
            started: std::time::Instant::now(),
        }
    }

    fn nonce_aad(&self, epoch: u32, seq: u64) -> ([u8; 12], Vec<u8>) {
        let mut nonce = [0u8; 12];
        nonce[..4].copy_from_slice(&self.direction.to_be_bytes());
        nonce[4..].copy_from_slice(&seq.to_be_bytes());
        let mut aad = Vec::with_capacity(LABEL_FRAME.len() + 16);
        aad.extend_from_slice(LABEL_FRAME.as_bytes());
        aad.extend_from_slice(&self.direction.to_be_bytes());
        aad.extend_from_slice(&epoch.to_be_bytes());
        aad.extend_from_slice(&seq.to_be_bytes());
        (nonce, aad)
    }

    fn advance(&mut self) {
        let next = next_epoch_key(&self.key);
        self.key.zeroize();
        self.key = next;
        self.cipher = Aes256Gcm::new(&self.key.into());
        self.epoch += 1;
        self.seq = 0;
        self.started = std::time::Instant::now();
    }

    fn needs_rekey(&self) -> bool {
        self.seq >= REKEY_AFTER_MESSAGES || self.started.elapsed() >= REKEY_AFTER
    }
}

impl Drop for Directional {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct EncryptedFrame {
    pub t: String,
    pub e: u32,
    pub s: u64,
    pub d: String,
}

pub enum Role {
    Client,
    Server,
}

pub struct SecureChannel {
    tx: Directional,
    rx: Directional,
}

impl SecureChannel {
    pub fn new(keys: &SessionKeys, role: Role) -> Self {
        let (tx, rx) = match role {
            Role::Server => (
                Directional::new(keys.s2c, DIR_S2C),
                Directional::new(keys.c2s, DIR_C2S),
            ),
            Role::Client => (
                Directional::new(keys.c2s, DIR_C2S),
                Directional::new(keys.s2c, DIR_S2C),
            ),
        };
        Self { tx, rx }
    }

    fn encrypt_one(&mut self, plain: &[u8]) -> EncryptedFrame {
        let (nonce, aad) = self.tx.nonce_aad(self.tx.epoch, self.tx.seq);
        let mut data = plain.to_vec();
        self.tx
            .cipher
            .encrypt_in_place(&nonce.into(), &aad, &mut data)
            .expect("aes-gcm encrypt");
        let frame = EncryptedFrame {
            t: "enc".into(),
            e: self.tx.epoch,
            s: self.tx.seq,
            d: b64u(&data),
        };
        self.tx.seq += 1;
        frame
    }

    /// Encrypts one JSON message; may append a rekey frame when the epoch is exhausted.
    pub fn seal(&mut self, message: &serde_json::Value) -> Vec<EncryptedFrame> {
        let plain = serde_json::to_vec(message).expect("json serialize");
        let mut frames = vec![self.encrypt_one(&plain)];
        if self.tx.needs_rekey() {
            let rekey = serde_json::json!({ "t": "rekey", "epoch": self.tx.epoch + 1 });
            frames.push(self.encrypt_one(&serde_json::to_vec(&rekey).unwrap()));
            self.tx.advance();
        }
        frames
    }

    /// Decrypts a frame. Returns `Ok(None)` when the frame was an internal rekey.
    pub fn open(
        &mut self,
        frame: &EncryptedFrame,
    ) -> Result<Option<serde_json::Value>, ChannelError> {
        if frame.t != "enc" {
            return Err(ChannelError::Malformed);
        }
        if frame.e != self.rx.epoch || frame.s != self.rx.seq {
            return Err(ChannelError::Sequence);
        }
        let mut data = crate::util::from_b64u(&frame.d, None).ok_or(ChannelError::Malformed)?;
        if data.len() < 16 || data.len() > 64 * 1024 + 16 {
            return Err(ChannelError::Malformed);
        }
        let (nonce, aad) = self.rx.nonce_aad(frame.e, frame.s);
        self.rx
            .cipher
            .decrypt_in_place(&nonce.into(), &aad, &mut data)
            .map_err(|_| ChannelError::Auth)?;
        let plain = data;
        self.rx.seq += 1;
        let value: serde_json::Value =
            serde_json::from_slice(&plain).map_err(|_| ChannelError::Malformed)?;
        if value.get("t").and_then(|t| t.as_str()) == Some("rekey") {
            let epoch = value.get("epoch").and_then(|e| e.as_u64());
            if epoch != Some(self.rx.epoch as u64 + 1) {
                return Err(ChannelError::Malformed);
            }
            self.rx.advance();
            return Ok(None);
        }
        Ok(Some(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdh::EphemeralSecret;
    use rand::rngs::OsRng;

    #[test]
    fn channel_roundtrip_rejects_replay_and_tamper() {
        let keys = derive_session_keys(&[7u8; 32], [9u8; 32]);
        let mut server = SecureChannel::new(&keys, Role::Server);
        let mut client = SecureChannel::new(&keys, Role::Client);
        let frames = client.seal(&serde_json::json!({"t": "ping"}));
        let got = server.open(&frames[0]).unwrap().unwrap();
        assert_eq!(got["t"], "ping");
        assert_eq!(server.open(&frames[0]), Err(ChannelError::Sequence));
        let mut next = client.seal(&serde_json::json!({"t": "x"})).remove(0);
        let mut raw = crate::util::from_b64u(&next.d, None).unwrap();
        raw[0] ^= 1;
        next.d = b64u(&raw);
        assert_eq!(server.open(&next), Err(ChannelError::Auth));
    }

    #[test]
    fn ecdh_and_signatures() {
        let a = EphemeralSecret::random(&mut OsRng);
        let b = EphemeralSecret::random(&mut OsRng);
        let sa = a.diffie_hellman(&b.public_key());
        let sb = b.diffie_hellman(&a.public_key());
        assert_eq!(
            sa.raw_secret_bytes().as_slice(),
            sb.raw_secret_bytes().as_slice()
        );
        let key = SigningKey::random(&mut OsRng);
        let sig = sign(&key, LABEL_AGENT_SIG, b"hello");
        assert!(verify(key.verifying_key(), LABEL_AGENT_SIG, b"hello", &sig));
        assert!(!verify(
            key.verifying_key(),
            LABEL_DEVICE_SIG,
            b"hello",
            &sig
        ));
    }

    #[test]
    fn sas_format() {
        assert_eq!(sas_from_bytes([0; 8]), "000000");
        assert_eq!(sas_from_bytes(1_234_567u64.to_be_bytes()), "234567");
    }
}
