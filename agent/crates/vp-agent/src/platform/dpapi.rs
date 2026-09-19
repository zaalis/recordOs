//! Secrets at rest are wrapped with DPAPI (current user scope) plus an application entropy value,
//! so copying the data directory to another account or machine does not expose keys.

use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

const ENTROPY_PREFIX: &[u8] = b"Vision-Pro/1 dpapi:";

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    }
}

pub fn protect(data: &[u8], purpose: &str) -> anyhow::Result<Vec<u8>> {
    let entropy = [ENTROPY_PREFIX, purpose.as_bytes()].concat();
    let input = blob(data);
    let entropy_blob = blob(&entropy);
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            windows::core::w!("Vision-Pro"),
            Some(&entropy_blob),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )?;
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut core::ffi::c_void)));
        Ok(result)
    }
}

pub fn unprotect(data: &[u8], purpose: &str) -> anyhow::Result<Vec<u8>> {
    let entropy = [ENTROPY_PREFIX, purpose.as_bytes()].concat();
    let input = blob(data);
    let entropy_blob = blob(&entropy);
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            Some(&entropy_blob),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )?;
        let slice = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let result = slice.to_vec();
        slice.fill(0);
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut core::ffi::c_void)));
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn roundtrip() {
        let secret = b"top secret key material";
        let wrapped = super::protect(secret, "test").unwrap();
        assert_ne!(wrapped.as_slice(), secret);
        assert_eq!(super::unprotect(&wrapped, "test").unwrap(), secret);
        assert!(super::unprotect(&wrapped, "other-purpose").is_err());
    }
}
