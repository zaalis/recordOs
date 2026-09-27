use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use parking_lot::Mutex;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub fn b64u(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn from_b64u(text: &str, expected_len: usize) -> Option<Vec<u8>> {
    if text.len() > 256 {
        return None;
    }
    URL_SAFE_NO_PAD
        .decode(text)
        .ok()
        .filter(|bytes| bytes.len() == expected_len)
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    rand::rngs::OsRng.fill_bytes(&mut out);
    out
}

pub fn random_token(bytes: usize) -> String {
    let mut out = vec![0u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut out);
    b64u(&out)
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("hmac accepts any key");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

/// Same derivation as the agent: base64url(SHA-256(pub)[0..16]).
pub fn agent_id_from_public_key(public_raw: &[u8]) -> String {
    b64u(&Sha256::digest(public_raw)[..16])
}

pub fn is_agent_id(text: &str) -> bool {
    text.len() == 22
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Per-source token bucket. Bounded: when full, stale sources are evicted first and new
/// sources are refused under a flood.
pub struct RateLimiter {
    burst: f64,
    per_sec: f64,
    buckets: Mutex<HashMap<IpAddr, (f64, Instant)>>,
}

const MAX_SOURCES: usize = 16_384;

impl RateLimiter {
    pub fn new(burst: u32, per_sec: f64) -> RateLimiter {
        RateLimiter {
            burst: burst as f64,
            per_sec,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub fn allow(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let mut buckets = self.buckets.lock();
        if buckets.len() >= MAX_SOURCES && !buckets.contains_key(&ip) {
            let full_after = self.burst / self.per_sec;
            buckets.retain(|_, (_, at)| now.duration_since(*at).as_secs_f64() < full_after);
            if buckets.len() >= MAX_SOURCES {
                return false;
            }
        }
        let (tokens, at) = buckets.entry(ip).or_insert((self.burst, now));
        *tokens = (*tokens + now.duration_since(*at).as_secs_f64() * self.per_sec).min(self.burst);
        *at = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_allows_burst_then_refuses() {
        let limiter = RateLimiter::new(3, 0.001);
        let ip: IpAddr = "203.0.113.9".parse().unwrap();
        assert!((0..3).all(|_| limiter.allow(ip)));
        assert!(!limiter.allow(ip));
        assert!(limiter.allow("203.0.113.10".parse().unwrap()));
    }

    #[test]
    fn agent_id_shape() {
        let id = agent_id_from_public_key(&[4u8; 65]);
        assert!(is_agent_id(&id));
        assert!(!is_agent_id("../../etc/passwd000000"));
    }
}
