//! Secrets at rest: files in the data directory are sealed with AES-256-GCM under a random
//! 256-bit master key that lives only in the user's login Keychain. Copying the data directory to
//! another account or Mac therefore does not expose the agent identity or the local authority.
//!
//! The purpose string is bound as associated data, so a blob sealed for one purpose can never be
//! opened as another.

use super::ffi;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use parking_lot::Mutex;
use std::ffi::CString;
use zeroize::Zeroizing;

const SERVICE: &str = "com.vision-pro.agent";
const ACCOUNT: &str = "master-key-v1";
const MAGIC: &[u8; 4] = b"VPK1";
const AAD_PREFIX: &[u8] = b"Vision-Pro/1 keychain:";

const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

/// The Keychain refused or could not provide the master key (locked keychain, access denied,
/// no user session). Callers must not treat this as corrupted data: the secrets are intact.
#[derive(Debug, thiserror::Error)]
#[error("the macOS Keychain did not provide the Vision-Pro key (OSStatus {status}). Unlock the login keychain and allow access for Vision-Pro, then try again.")]
pub struct KeychainUnavailable {
    pub status: i32,
}

static MASTER: Mutex<Option<Zeroizing<[u8; 32]>>> = Mutex::new(None);

fn service_account() -> (CString, CString) {
    // VISION_PRO_HOME isolates test and development installs from the real agent key.
    let account = match std::env::var("VISION_PRO_HOME") {
        Ok(home) if !home.is_empty() => {
            let digest = crate::crypto::sha256(&[home.as_bytes()]);
            format!(
                "{ACCOUNT}-{:02x}{:02x}{:02x}{:02x}",
                digest[0], digest[1], digest[2], digest[3]
            )
        }
        _ => ACCOUNT.to_string(),
    };
    (
        CString::new(SERVICE).expect("service name"),
        CString::new(account).expect("account name"),
    )
}

fn master_key() -> anyhow::Result<Zeroizing<[u8; 32]>> {
    let mut cache = MASTER.lock();
    if let Some(key) = cache.as_ref() {
        return Ok(key.clone());
    }
    let (service, account) = service_account();
    let mut key = Zeroizing::new([0u8; 32]);
    let mut len = 0usize;
    let status = unsafe {
        ffi::vp_keychain_get(
            service.as_ptr(),
            account.as_ptr(),
            key.as_mut_ptr(),
            key.len(),
            &mut len,
        )
    };
    match status {
        0 if len == 32 => {}
        0 => anyhow::bail!("the Vision-Pro Keychain item has an unexpected size"),
        ERR_SEC_ITEM_NOT_FOUND => {
            key.copy_from_slice(&crate::util::random_bytes::<32>());
            let stored = unsafe {
                ffi::vp_keychain_set(service.as_ptr(), account.as_ptr(), key.as_ptr(), key.len())
            };
            if stored != 0 {
                return Err(KeychainUnavailable { status: stored }.into());
            }
            tracing::info!("created the Vision-Pro master key in the login Keychain");
        }
        status => return Err(KeychainUnavailable { status }.into()),
    }
    *cache = Some(key.clone());
    Ok(key)
}

fn cipher() -> anyhow::Result<Aes256Gcm> {
    let key = master_key()?;
    Ok(Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref())))
}

fn aad(purpose: &str) -> Vec<u8> {
    [AAD_PREFIX, purpose.as_bytes()].concat()
}

pub fn protect(data: &[u8], purpose: &str) -> anyhow::Result<Vec<u8>> {
    let cipher = cipher()?;
    let nonce_bytes = crate::util::random_bytes::<12>();
    let aad = aad(purpose);
    let sealed = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: data,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;
    let mut out = Vec::with_capacity(MAGIC.len() + nonce_bytes.len() + sealed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&sealed);
    Ok(out)
}

pub fn unprotect(data: &[u8], purpose: &str) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        data.len() > MAGIC.len() + 12 + 16 && &data[..4] == MAGIC,
        "not a Vision-Pro sealed file"
    );
    let cipher = cipher()?;
    let (nonce, sealed) = data[4..].split_at(12);
    let aad = aad(purpose);
    cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: sealed,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("sealed file is corrupted or belongs to another Keychain"))
}

/// True when the error means "the Keychain is unavailable right now" rather than "bad data".
pub fn is_unavailable(error: &anyhow::Error) -> bool {
    error.downcast_ref::<KeychainUnavailable>().is_some()
}

/// Removes the master key (used by tests; a real reset replaces the identity instead).
pub fn delete_master_key() {
    let (service, account) = service_account();
    unsafe {
        ffi::vp_keychain_delete(service.as_ptr(), account.as_ptr());
    }
    *MASTER.lock() = None;
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "touches the login Keychain; run with `cargo test -- --ignored` on a Mac"]
    fn roundtrip() {
        std::env::set_var(
            "VISION_PRO_HOME",
            std::env::temp_dir().join("vp-keychain-test"),
        );
        let secret = b"top secret key material";
        let wrapped = super::protect(secret, "test").unwrap();
        assert_ne!(&wrapped[4..], secret);
        assert_eq!(super::unprotect(&wrapped, "test").unwrap(), secret);
        assert!(super::unprotect(&wrapped, "other-purpose").is_err());
        super::delete_master_key();
    }
}
