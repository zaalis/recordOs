use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn b64u(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Strict base64url decoding with an optional exact length requirement.
pub fn from_b64u(text: &str, expected_len: Option<usize>) -> Option<Vec<u8>> {
    if text.len() > 16 * 1024 * 1024
        || !text
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
    match expected_len {
        Some(len) if bytes.len() != len => None,
        _ => Some(bytes),
    }
}

/// Length-prefixed field (u32 big-endian length || bytes), identical to the TS `lp()` helper.
pub fn lp(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    use rand::RngCore;
    let mut out = [0u8; N];
    rand::rngs::OsRng.fill_bytes(&mut out);
    out
}

pub fn random_token(bytes: usize) -> String {
    use rand::RngCore;
    let mut out = vec![0u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut out);
    b64u(&out)
}

/// Uniform random 4-digit code (rejection sampling, no modulo bias).
pub fn random_code() -> String {
    use rand::RngCore;
    loop {
        let v = rand::rngs::OsRng.next_u32();
        let limit = u32::MAX - (u32::MAX % 10_000);
        if v < limit {
            return format!("{:04}", v % 10_000);
        }
    }
}

/// Keeps user-provided names printable, single-line and bounded.
pub fn sanitize_name(input: &str, max_chars: usize) -> String {
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_control() && !matches!(*c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        .take(max_chars)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "Unnamed device".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b64u_roundtrip_and_strictness() {
        let data = random_bytes::<33>();
        assert_eq!(from_b64u(&b64u(&data), Some(33)).unwrap(), data);
        assert!(from_b64u("ab+c", None).is_none());
        assert!(from_b64u("abc=", None).is_none());
    }

    #[test]
    fn names_are_sanitized() {
        assert_eq!(
            sanitize_name("  Bryan's\u{202E} iPad\n ", 48),
            "Bryan's iPad"
        );
        assert_eq!(sanitize_name("\n", 48), "Unnamed device");
        assert_eq!(random_code().len(), 4);
    }
}
