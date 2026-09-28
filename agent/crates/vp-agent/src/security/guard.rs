//! Brute-force protection: per-IP connection token buckets, failure counting with exponential
//! lockouts, and pairing-denial throttling. All state is in memory and bounded.

use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::time::{Duration, Instant};

const FAILURE_WINDOW: Duration = Duration::from_secs(15 * 60);
const DENIAL_WINDOW: Duration = Duration::from_secs(60 * 60);
const MAX_TRACKED_IPS: usize = 4096;
// This bucket covers every TCP connection, including page assets, API reads
// and signaling. Two quick Safari page loads plus reconnects must fit without
// being mistaken for a flood. Authentication failures have a separate lockout.
// A client that retries immediately on every rejection (no backoff) must not
// be able to keep itself locked out forever by consuming tokens as fast as
// they refill: the burst and the refill rate both have headroom above what a
// reconnect loop plausibly sustains.
const CONN_BURST: f64 = 60.0;
const CONN_PER_SEC: f64 = 10.0;
const MAX_LOCKOUT: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    RateLimited(Duration),
    LockedOut(Duration),
}

struct IpState {
    tokens: f64,
    refilled: Instant,
    failures: VecDeque<Instant>,
    denials: VecDeque<Instant>,
    lockouts: u32,
    locked_until: Option<Instant>,
    last_activity: Instant,
}

impl IpState {
    fn new(now: Instant) -> Self {
        Self {
            tokens: CONN_BURST,
            refilled: now,
            failures: VecDeque::new(),
            denials: VecDeque::new(),
            lockouts: 0,
            locked_until: None,
            last_activity: now,
        }
    }
}

pub struct Guard {
    state: Mutex<HashMap<IpAddr, IpState>>,
    threshold: Mutex<u32>,
    base_lockout: Mutex<Duration>,
}

impl Guard {
    pub fn new(threshold: u32, lockout_minutes: u32) -> Guard {
        Guard {
            state: Mutex::new(HashMap::new()),
            threshold: Mutex::new(threshold),
            base_lockout: Mutex::new(Duration::from_secs(lockout_minutes as u64 * 60)),
        }
    }

    pub fn configure(&self, threshold: u32, lockout_minutes: u32) {
        *self.threshold.lock() = threshold;
        *self.base_lockout.lock() = Duration::from_secs(lockout_minutes as u64 * 60);
    }

    fn with_ip<R>(&self, ip: IpAddr, f: impl FnOnce(&mut IpState, Instant) -> R) -> R {
        let now = Instant::now();
        let mut map = self.state.lock();
        if map.len() >= MAX_TRACKED_IPS && !map.contains_key(&ip) {
            map.retain(|_, s| {
                s.locked_until.is_some_and(|t| t > now)
                    || now.duration_since(s.last_activity) < FAILURE_WINDOW
            });
            if map.len() >= MAX_TRACKED_IPS {
                // Under a flood of distinct sources, fail closed for new sources.
                let mut state = IpState::new(now);
                state.tokens = 0.0;
                return f(&mut state, now);
            }
        }
        let state = map.entry(ip).or_insert_with(|| IpState::new(now));
        state.last_activity = now;
        f(state, now)
    }

    /// Called for every new TCP/WebSocket connection.
    pub fn check_connection(&self, ip: IpAddr) -> Verdict {
        self.with_ip(ip, |s, now| {
            if let Some(until) = s.locked_until {
                if until > now {
                    return Verdict::LockedOut(until - now);
                }
                s.locked_until = None;
            }
            let elapsed = now.duration_since(s.refilled).as_secs_f64();
            s.tokens = (s.tokens + elapsed * CONN_PER_SEC).min(CONN_BURST);
            s.refilled = now;
            if s.tokens >= 1.0 {
                s.tokens -= 1.0;
                Verdict::Allow
            } else {
                Verdict::RateLimited(Duration::from_secs_f64((1.0 - s.tokens) / CONN_PER_SEC))
            }
        })
    }

    pub fn locked_for(&self, ip: IpAddr) -> Option<Duration> {
        self.with_ip(ip, |s, now| {
            s.locked_until.filter(|t| *t > now).map(|t| t - now)
        })
    }

    /// Records an authentication/protocol failure. Returns the lockout duration if one was triggered.
    pub fn record_failure(&self, ip: IpAddr) -> Option<Duration> {
        let threshold = *self.threshold.lock() as usize;
        let base = *self.base_lockout.lock();
        self.with_ip(ip, |s, now| {
            s.failures.push_back(now);
            while s
                .failures
                .front()
                .is_some_and(|t| now.duration_since(*t) > FAILURE_WINDOW)
            {
                s.failures.pop_front();
            }
            if s.failures.len() >= threshold {
                let factor = 1u32 << s.lockouts.min(10);
                let duration = (base * factor).min(MAX_LOCKOUT);
                s.lockouts += 1;
                s.failures.clear();
                s.locked_until = Some(now + duration);
                Some(duration)
            } else {
                None
            }
        })
    }

    pub fn record_success(&self, ip: IpAddr) {
        self.with_ip(ip, |s, _| {
            s.failures.clear();
        });
    }

    /// Three denied pairing requests within an hour lock the source for an hour.
    pub fn record_pair_denied(&self, ip: IpAddr) -> Option<Duration> {
        self.with_ip(ip, |s, now| {
            s.denials.push_back(now);
            while s
                .denials
                .front()
                .is_some_and(|t| now.duration_since(*t) > DENIAL_WINDOW)
            {
                s.denials.pop_front();
            }
            if s.denials.len() >= 3 {
                s.denials.clear();
                s.locked_until = Some(now + DENIAL_WINDOW);
                Some(DENIAL_WINDOW)
            } else {
                None
            }
        })
    }

    pub fn unlock_all(&self) {
        self.state.lock().clear();
    }

    pub fn locked_sources(&self) -> Vec<(IpAddr, Duration)> {
        let now = Instant::now();
        self.state
            .lock()
            .iter()
            .filter_map(|(ip, s)| s.locked_until.filter(|t| *t > now).map(|t| (*ip, t - now)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockout_after_threshold_and_escalates() {
        let guard = Guard::new(3, 1);
        let ip: IpAddr = "192.168.1.20".parse().unwrap();
        assert_eq!(guard.record_failure(ip), None);
        assert_eq!(guard.record_failure(ip), None);
        assert_eq!(guard.record_failure(ip), Some(Duration::from_secs(60)));
        assert!(matches!(guard.check_connection(ip), Verdict::LockedOut(_)));
        guard.state.lock().get_mut(&ip).unwrap().locked_until = None;
        guard.record_failure(ip);
        guard.record_failure(ip);
        assert_eq!(guard.record_failure(ip), Some(Duration::from_secs(120)));
    }

    #[test]
    fn connection_bucket_limits_bursts() {
        let guard = Guard::new(5, 15);
        let ip: IpAddr = "10.0.0.2".parse().unwrap();
        let allowed = (0..(CONN_BURST as u32 * 2))
            .filter(|_| guard.check_connection(ip) == Verdict::Allow)
            .count();
        assert_eq!(allowed, CONN_BURST as usize);
    }

    #[test]
    fn normal_page_reload_and_signaling_retries_fit_connection_bucket() {
        let guard = Guard::new(5, 15);
        let ip: IpAddr = "10.0.0.3".parse().unwrap();
        // Each page load can use several TCP sockets for HTML, JS, CSS, API
        // and service-worker requests before the WebSocket is established.
        for _ in 0..24 {
            assert_eq!(guard.check_connection(ip), Verdict::Allow);
        }
    }
}
