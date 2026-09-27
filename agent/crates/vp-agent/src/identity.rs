//! Long-term agent identity (ECDSA P-256) and local integrity key, sealed at rest with a key held
//! in the macOS login Keychain.

use crate::crypto;
use crate::paths;
use crate::platform::keychain;
use crate::util::{b64u, from_b64u, now_ms, random_bytes};
use p256::ecdsa::{SigningKey, VerifyingKey};
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const PURPOSE: &str = "agent-identity";

#[derive(Serialize, Deserialize)]
struct StoredIdentity {
    version: u32,
    signing_key: String,
    integrity_key: String,
    created_at: u64,
}

pub struct AgentIdentity {
    signing_key: SigningKey,
    public_raw: [u8; 65],
    pub agent_id: String,
    pub fingerprint: String,
    integrity_key: Zeroizing<[u8; 32]>,
    pub created_at: u64,
}

impl AgentIdentity {
    pub fn load_or_create() -> anyhow::Result<AgentIdentity> {
        let dir = paths::ensure_data_dir()?;
        let path = dir.join("identity.bin");
        if let Ok(wrapped) = std::fs::read(&path) {
            match Self::decode(&wrapped) {
                Ok(identity) => return Ok(identity),
                // A locked or refused Keychain is not corruption: never replace the identity
                // (which would silently unpair every iPad) — report it and let the user retry.
                Err(error) if keychain::is_unavailable(&error) => return Err(error),
                Err(error) => {
                    // Never silently replace an identity: keep the unreadable file for inspection.
                    tracing::error!(%error, "agent identity unreadable; creating a new identity (all devices must pair again)");
                    let _ = std::fs::rename(
                        &path,
                        dir.join(format!("identity.unreadable-{}.bin", now_ms())),
                    );
                }
            }
        }
        let identity = Self::generate();
        identity.persist()?;
        Ok(identity)
    }

    fn generate() -> AgentIdentity {
        let signing_key = SigningKey::random(&mut rand::rngs::OsRng);
        Self::from_parts(signing_key, Zeroizing::new(random_bytes::<32>()), now_ms())
    }

    fn from_parts(
        signing_key: SigningKey,
        integrity_key: Zeroizing<[u8; 32]>,
        created_at: u64,
    ) -> AgentIdentity {
        let public_raw = crypto::public_raw(signing_key.verifying_key());
        AgentIdentity {
            agent_id: crypto::id_from_public_key(&public_raw),
            fingerprint: crypto::fingerprint_of(&public_raw),
            public_raw,
            signing_key,
            integrity_key,
            created_at,
        }
    }

    fn decode(wrapped: &[u8]) -> anyhow::Result<AgentIdentity> {
        let plain = Zeroizing::new(keychain::unprotect(wrapped, PURPOSE)?);
        let stored: StoredIdentity = serde_json::from_slice(&plain)?;
        anyhow::ensure!(stored.version == 1, "unsupported identity version");
        let der = Zeroizing::new(
            from_b64u(&stored.signing_key, None)
                .ok_or_else(|| anyhow::anyhow!("bad key encoding"))?,
        );
        let signing_key =
            SigningKey::from_pkcs8_der(&der).map_err(|e| anyhow::anyhow!("bad key: {e}"))?;
        let integrity = from_b64u(&stored.integrity_key, Some(32))
            .ok_or_else(|| anyhow::anyhow!("bad integrity key"))?;
        let mut key = Zeroizing::new([0u8; 32]);
        key.copy_from_slice(&integrity);
        Ok(Self::from_parts(signing_key, key, stored.created_at))
    }

    fn persist(&self) -> anyhow::Result<()> {
        let der = self
            .signing_key
            .to_pkcs8_der()
            .map_err(|e| anyhow::anyhow!("encode key: {e}"))?;
        let stored = StoredIdentity {
            version: 1,
            signing_key: b64u(der.as_bytes()),
            integrity_key: b64u(self.integrity_key.as_ref()),
            created_at: self.created_at,
        };
        let plain = Zeroizing::new(serde_json::to_vec(&stored)?);
        let wrapped = keychain::protect(&plain, PURPOSE)?;
        paths::write_atomic(&paths::data_dir().join("identity.bin"), &wrapped)?;
        Ok(())
    }

    /// Replaces the identity. Every paired device becomes invalid because pinned keys no longer match.
    pub fn reset() -> anyhow::Result<AgentIdentity> {
        let identity = Self::generate();
        identity.persist()?;
        Ok(identity)
    }

    pub fn public_raw(&self) -> &[u8; 65] {
        &self.public_raw
    }

    pub fn verifying_key(&self) -> &VerifyingKey {
        self.signing_key.verifying_key()
    }

    pub fn sign(&self, label: &str, data: &[u8]) -> [u8; 64] {
        crypto::sign(&self.signing_key, label, data)
    }

    /// HMAC-SHA256 with the local integrity key (detects offline tampering of local stores).
    pub fn integrity_mac(&self, data: &[u8]) -> [u8; 32] {
        use hmac::{Hmac, Mac};
        let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(self.integrity_key.as_ref())
            .expect("hmac key");
        mac.update(b"Vision-Pro/1 store:");
        mac.update(data);
        mac.finalize().into_bytes().into()
    }

    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }
}
