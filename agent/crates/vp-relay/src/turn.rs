//! TURN server (RFC 8656) whose only reachable peers are registered Vision-Pro agents.
//!
//! Credentials are issued by the relay to an authenticated agent, which forwards them to its
//! iPad inside the end-to-end encrypted signaling channel. Each credential is bound to one
//! virtual peer address leased to that agent: permissions, channels and indications towards any
//! other address are refused, so the relay can never be used to reach arbitrary hosts.

use crate::stun::{self, attr, method, Builder, Class, Message};
use crate::util::{b64u, hmac_sha256, now_unix, random_bytes, random_token};
use crate::wire;
use crate::Relay;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

pub const REALM: &str = "vision-pro";
const SOFTWARE: &[u8] = b"Vision-Pro relay";
const DEFAULT_LIFETIME: u32 = 600;
const MAX_LIFETIME: u32 = 3600;
const PERMISSION_LIFETIME: Duration = Duration::from_secs(300);
const CHANNEL_LIFETIME: Duration = Duration::from_secs(600);
const NONCE_LIFETIME: u64 = 3600;
const MAX_ALLOCATIONS: usize = 20_000;
const MAX_ALLOCATIONS_PER_IP: usize = 16;
const MAX_ALLOCATIONS_PER_LEASE: usize = 6;
const MAX_LEASES_PER_AGENT: usize = 32;
const MIN_CREDENTIAL_TTL: u64 = 60;
const MAX_CREDENTIAL_TTL: u64 = 8 * 24 * 3600;
const TRANSPORT_UDP: u8 = 17;

/// Comprehension-required attributes this server understands (or safely ignores).
const KNOWN_REQUIRED: &[u16] = &[
    attr::MAPPED_ADDRESS,
    attr::USERNAME,
    attr::MESSAGE_INTEGRITY,
    attr::ERROR_CODE,
    attr::CHANNEL_NUMBER,
    attr::LIFETIME,
    attr::XOR_PEER_ADDRESS,
    attr::DATA,
    attr::REALM,
    attr::NONCE,
    attr::REQUESTED_ADDRESS_FAMILY,
    attr::EVEN_PORT,
    attr::REQUESTED_TRANSPORT,
    attr::DONT_FRAGMENT,
    attr::MESSAGE_INTEGRITY_SHA256,
    attr::USERHASH,
    attr::XOR_MAPPED_ADDRESS,
    attr::PRIORITY,
    attr::USE_CANDIDATE,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientKey {
    Udp(SocketAddr),
    Stream(u64),
}

#[derive(Clone)]
pub enum Sink {
    Udp(Arc<UdpSocket>, SocketAddr),
    Stream(mpsc::Sender<Vec<u8>>),
}

impl Sink {
    fn send(&self, data: Vec<u8>) {
        match self {
            Sink::Udp(socket, addr) => {
                let _ = socket.try_send_to(&data, *addr);
            }
            // Media semantics: under backpressure newer packets are dropped rather than queued.
            Sink::Stream(tx) => {
                let _ = tx.try_send(data);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Credential {
    pub username: String,
    pub password: String,
    pub peer: SocketAddrV4,
    pub expires_unix: u64,
}

struct Lease {
    agent_id: String,
    conn_id: u64,
    token: String,
    expires_unix: u64,
}

struct Allocation {
    client_addr: SocketAddr,
    relay: SocketAddrV4,
    username: String,
    peer: SocketAddrV4,
    lifetime: u32,
    expires: Instant,
    permissions: HashMap<Ipv4Addr, Instant>,
    channels: HashMap<u16, (SocketAddrV4, Instant)>,
    sink: Sink,
    allocate_txid: [u8; 12],
}

impl Allocation {
    fn permitted(&self, peer: SocketAddrV4, now: Instant) -> bool {
        peer == self.peer
            && self
                .permissions
                .get(peer.ip())
                .is_some_and(|until| *until > now)
    }

    fn channel_for(&self, peer: SocketAddrV4, now: Instant) -> Option<u16> {
        self.channels
            .iter()
            .find(|(_, (p, until))| *p == peer && *until > now)
            .map(|(n, _)| *n)
    }
}

#[derive(Default)]
struct Tables {
    allocations: HashMap<ClientKey, Allocation>,
    by_relay: HashMap<SocketAddrV4, ClientKey>,
    leases: HashMap<SocketAddrV4, Lease>,
    lease_tokens: HashMap<String, SocketAddrV4>,
    next_relay: u32,
    next_peer: u32,
}

impl Tables {
    fn remove_allocation(&mut self, key: &ClientKey) {
        if let Some(allocation) = self.allocations.remove(key) {
            self.by_relay.remove(&allocation.relay);
        }
    }

    fn remove_lease(&mut self, peer: &SocketAddrV4) {
        if let Some(lease) = self.leases.remove(peer) {
            self.lease_tokens.remove(&lease.token);
        }
        let keys: Vec<ClientKey> = self
            .allocations
            .iter()
            .filter(|(_, a)| a.peer == *peer)
            .map(|(k, _)| *k)
            .collect();
        for key in keys {
            self.remove_allocation(&key);
        }
    }
}

pub struct Turn {
    secret: [u8; 32],
    nonce_key: [u8; 32],
    tables: Mutex<Tables>,
}

impl Default for Turn {
    fn default() -> Self {
        Self::new()
    }
}

impl Turn {
    pub fn new() -> Turn {
        Turn {
            secret: random_bytes(),
            nonce_key: random_bytes(),
            tables: Mutex::new(Tables {
                next_relay: u32::from_be_bytes(random_bytes()) & 0xFFFF,
                ..Tables::default()
            }),
        }
    }

    fn password(&self, username: &str) -> String {
        b64u(&hmac_sha256(&self.secret, username.as_bytes()))
    }

    // --- Leases (called for authenticated agents) -------------------------------------------

    pub fn issue(&self, agent_id: &str, conn_id: u64, ttl_secs: u64) -> Option<Credential> {
        let mut t = self.tables.lock();
        if t.leases.values().filter(|l| l.agent_id == agent_id).count() >= MAX_LEASES_PER_AGENT {
            return None;
        }
        let peer = loop {
            t.next_peer = t.next_peer.wrapping_add(1);
            let n = t.next_peer;
            let host = (n & 0xFF) as u8;
            // Browsers ignore remote ICE candidates on ports below 1024 (except 53, 80 and 443).
            let port = 20_000 + (u16::from_be_bytes(random_bytes()) % 40_000);
            let addr = SocketAddrV4::new(
                Ipv4Addr::new(100, 64 + ((n >> 16) & 0x3F) as u8, (n >> 8) as u8, host),
                port,
            );
            if host != 0 && host != 255 && !t.leases.contains_key(&addr) {
                break addr;
            }
        };
        let token = random_token(18);
        let expires_unix = now_unix() + ttl_secs.clamp(MIN_CREDENTIAL_TTL, MAX_CREDENTIAL_TTL);
        let username = format!("{expires_unix}:{token}");
        let password = self.password(&username);
        t.lease_tokens.insert(token.clone(), peer);
        t.leases.insert(
            peer,
            Lease {
                agent_id: agent_id.to_string(),
                conn_id,
                token,
                expires_unix,
            },
        );
        Some(Credential {
            username,
            password,
            peer,
            expires_unix,
        })
    }

    pub fn release(&self, agent_id: &str, peer: SocketAddrV4) {
        let mut t = self.tables.lock();
        if t.leases.get(&peer).is_some_and(|l| l.agent_id == agent_id) {
            t.remove_lease(&peer);
        }
    }

    pub fn release_connection(&self, conn_id: u64) {
        let mut t = self.tables.lock();
        let peers: Vec<SocketAddrV4> = t
            .leases
            .iter()
            .filter(|(_, l)| l.conn_id == conn_id)
            .map(|(p, _)| *p)
            .collect();
        for peer in peers {
            t.remove_lease(&peer);
        }
    }

    pub fn drop_client(&self, key: ClientKey) {
        self.tables.lock().remove_allocation(&key);
    }

    pub fn sweep(&self) {
        let now = Instant::now();
        let unix = now_unix();
        let mut t = self.tables.lock();
        let expired: Vec<SocketAddrV4> = t
            .leases
            .iter()
            .filter(|(_, l)| l.expires_unix < unix)
            .map(|(p, _)| *p)
            .collect();
        for peer in expired {
            t.remove_lease(&peer);
        }
        let stale: Vec<ClientKey> = t
            .allocations
            .iter()
            .filter(|(_, a)| a.expires <= now)
            .map(|(k, _)| *k)
            .collect();
        for key in stale {
            t.remove_allocation(&key);
        }
        for allocation in t.allocations.values_mut() {
            allocation.permissions.retain(|_, until| *until > now);
            allocation.channels.retain(|_, (_, until)| *until > now);
        }
    }

    pub fn allocation_count(&self) -> usize {
        self.tables.lock().allocations.len()
    }

    // --- Agent → iPad ------------------------------------------------------------------------

    pub fn deliver_from_agent(
        &self,
        agent_id: &str,
        conn_id: u64,
        peer: SocketAddrV4,
        relay: SocketAddrV4,
        payload: &[u8],
    ) {
        let now = Instant::now();
        let t = self.tables.lock();
        if !t
            .leases
            .get(&peer)
            .is_some_and(|l| l.agent_id == agent_id && l.conn_id == conn_id)
        {
            return;
        }
        let Some(allocation) = t.by_relay.get(&relay).and_then(|k| t.allocations.get(k)) else {
            return;
        };
        if !allocation.permitted(peer, now) {
            return;
        }
        let frame = match allocation.channel_for(peer, now) {
            Some(channel) => {
                let stream = matches!(allocation.sink, Sink::Stream(_));
                let mut frame = Vec::with_capacity(4 + payload.len() + 3);
                frame.extend_from_slice(&channel.to_be_bytes());
                frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
                frame.extend_from_slice(payload);
                if stream {
                    // RFC 8656 §12.5: ChannelData over TCP/TLS is padded to a multiple of 4.
                    frame.resize(frame.len().div_ceil(4) * 4, 0);
                }
                frame
            }
            None => {
                let mut builder = Builder::new(method::DATA, Class::Indication, random_bytes());
                builder.xor_address(attr::XOR_PEER_ADDRESS, SocketAddr::V4(peer));
                builder.attr(attr::DATA, payload);
                builder.finish()
            }
        };
        allocation.sink.send(frame);
    }

    // --- iPad → relay ------------------------------------------------------------------------

    pub fn handle_client(
        &self,
        relay: &Relay,
        key: ClientKey,
        client_addr: SocketAddr,
        sink: &Sink,
        data: &[u8],
    ) {
        if data.len() < 4 {
            return;
        }
        if data[0] & 0xC0 == 0x40 {
            self.channel_data(relay, key, data);
            return;
        }
        let Ok(message) = Message::parse(data) else {
            return;
        };
        if !message.fingerprint_ok() {
            return;
        }
        match (message.method, message.class) {
            (method::BINDING, Class::Request) => {
                let mut b = Builder::new(method::BINDING, Class::Success, message.txid);
                b.xor_address(attr::XOR_MAPPED_ADDRESS, client_addr);
                b.attr(attr::SOFTWARE, SOFTWARE);
                b.fingerprint();
                sink.send(b.finish());
            }
            (method::ALLOCATE, Class::Request) => {
                if let Some(response) = self.allocate(key, client_addr, sink, &message) {
                    sink.send(response);
                }
            }
            (method::REFRESH, Class::Request) => sink.send(self.refresh(key, &message)),
            (method::CREATE_PERMISSION, Class::Request) => {
                sink.send(self.create_permission(key, &message))
            }
            (method::CHANNEL_BIND, Class::Request) => sink.send(self.channel_bind(key, &message)),
            (method::SEND, Class::Indication) => self.send_indication(relay, key, &message),
            (_, Class::Request) => sink.send(error(&message, 400, "Bad Request", None)),
            _ => {}
        }
    }

    fn channel_data(&self, relay: &Relay, key: ClientKey, data: &[u8]) {
        let channel = u16::from_be_bytes([data[0], data[1]]);
        let length = u16::from_be_bytes([data[2], data[3]]) as usize;
        if data.len() < 4 + length || length == 0 {
            return;
        }
        let now = Instant::now();
        let route = {
            let t = self.tables.lock();
            t.allocations
                .get(&key)
                .and_then(|a| {
                    let (peer, until) = a.channels.get(&channel)?;
                    (*until > now && a.permitted(*peer, now)).then_some((a.relay, *peer))
                })
                .and_then(|(relay_addr, peer)| {
                    t.leases
                        .get(&peer)
                        .map(|l| (l.agent_id.clone(), l.conn_id, relay_addr, peer))
                })
        };
        if let Some((agent_id, conn_id, relay_addr, peer)) = route {
            relay.hub.to_agent(
                &agent_id,
                conn_id,
                wire::encode_data(peer, relay_addr, &data[4..4 + length]),
            );
        }
    }

    fn send_indication(&self, relay: &Relay, key: ClientKey, message: &Message) {
        let (Some(SocketAddr::V4(peer)), Some(payload)) = (
            message.xor_address(attr::XOR_PEER_ADDRESS),
            message.get(attr::DATA),
        ) else {
            return;
        };
        if payload.is_empty() {
            return;
        }
        let now = Instant::now();
        let route = {
            let t = self.tables.lock();
            t.allocations
                .get(&key)
                .filter(|a| a.permitted(peer, now))
                .and_then(|a| {
                    t.leases
                        .get(&peer)
                        .map(|l| (l.agent_id.clone(), l.conn_id, a.relay))
                })
        };
        if let Some((agent_id, conn_id, relay_addr)) = route {
            relay.hub.to_agent(
                &agent_id,
                conn_id,
                wire::encode_data(peer, relay_addr, payload),
            );
        }
    }

    fn nonce(&self) -> String {
        let ts = now_unix();
        let mac = hmac_sha256(&self.nonce_key, &ts.to_be_bytes());
        format!("{ts:016x}{}", hex(&mac[..8]))
    }

    fn nonce_fresh(&self, nonce: &str) -> bool {
        if nonce.len() != 32 {
            return false;
        }
        let Ok(ts) = u64::from_str_radix(&nonce[..16], 16) else {
            return false;
        };
        let now = now_unix();
        if ts > now + 60 || now.saturating_sub(ts) > NONCE_LIFETIME {
            return false;
        }
        let expected = hex(&hmac_sha256(&self.nonce_key, &ts.to_be_bytes())[..8]);
        expected.as_bytes().ct_eq(&nonce.as_bytes()[16..]).into()
    }

    fn unauthorized(&self, message: &Message, code: u16, reason: &str) -> Vec<u8> {
        let mut b = Builder::new(message.method, Class::Error, message.txid);
        b.error(code, reason);
        b.attr(attr::REALM, REALM.as_bytes());
        b.attr(attr::NONCE, self.nonce().as_bytes());
        b.attr(attr::SOFTWARE, SOFTWARE);
        b.fingerprint();
        b.finish()
    }

    /// Long-term credential check. Returns the username, key and leased peer.
    fn authenticate(
        &self,
        message: &Message,
        expected_username: Option<&str>,
    ) -> Result<(String, [u8; 16], SocketAddrV4), Vec<u8>> {
        let (Some(username), Some(realm), Some(nonce), Some(_)) = (
            message.text(attr::USERNAME),
            message.text(attr::REALM),
            message.text(attr::NONCE),
            message.get(attr::MESSAGE_INTEGRITY),
        ) else {
            return Err(self.unauthorized(message, 401, "Unauthorized"));
        };
        if realm != REALM || username.len() > 128 {
            return Err(self.unauthorized(message, 401, "Unauthorized"));
        }
        let key = stun::long_term_key(username, REALM, &self.password(username));
        if !message.verify_integrity(&key) {
            return Err(self.unauthorized(message, 401, "Unauthorized"));
        }
        if !self.nonce_fresh(nonce) {
            let mut b = Builder::new(message.method, Class::Error, message.txid);
            b.error(438, "Stale Nonce");
            b.attr(attr::REALM, REALM.as_bytes());
            b.attr(attr::NONCE, self.nonce().as_bytes());
            b.integrity(&key);
            b.fingerprint();
            return Err(b.finish());
        }
        if expected_username.is_some_and(|expected| expected != username) {
            return Err(error(message, 441, "Wrong Credentials", Some(&key)));
        }
        let Some((expires, token)) = username.split_once(':') else {
            return Err(self.unauthorized(message, 401, "Unauthorized"));
        };
        let expires: u64 = expires.parse().unwrap_or(0);
        let t = self.tables.lock();
        let peer = t.lease_tokens.get(token).copied().filter(|peer| {
            t.leases
                .get(peer)
                .is_some_and(|l| l.expires_unix == expires)
        });
        match peer {
            Some(peer) if expires >= now_unix() => Ok((username.to_string(), key, peer)),
            _ => {
                drop(t);
                Err(self.unauthorized(message, 401, "Unauthorized"))
            }
        }
    }

    fn allocate(
        &self,
        key: ClientKey,
        client_addr: SocketAddr,
        sink: &Sink,
        message: &Message,
    ) -> Option<Vec<u8>> {
        let (username, cred_key, peer) = match self.authenticate(message, None) {
            Ok(v) => v,
            Err(response) => return Some(response),
        };
        let unknown = unknown_required(message);
        if !unknown.is_empty() {
            let mut b = Builder::new(method::ALLOCATE, Class::Error, message.txid);
            b.error(420, "Unknown Attribute");
            let list: Vec<u8> = unknown.iter().flat_map(|k| k.to_be_bytes()).collect();
            b.attr(attr::UNKNOWN_ATTRIBUTES, &list);
            b.integrity(&cred_key);
            b.fingerprint();
            return Some(b.finish());
        }
        let mut t = self.tables.lock();
        if let Some(existing) = t.allocations.get(&key) {
            // A retransmitted Allocate receives the original answer; anything else is a mismatch.
            if existing.allocate_txid == message.txid && existing.username == username {
                return Some(allocate_success(
                    message.txid,
                    existing.relay,
                    existing.client_addr,
                    existing.lifetime,
                    &cred_key,
                ));
            }
            return Some(error(message, 437, "Allocation Mismatch", Some(&cred_key)));
        }
        match message.get(attr::REQUESTED_TRANSPORT) {
            Some(v) if v.len() == 4 && v[0] == TRANSPORT_UDP => {}
            Some(_) => {
                return Some(error(
                    message,
                    442,
                    "Unsupported Transport Protocol",
                    Some(&cred_key),
                ))
            }
            None => return Some(error(message, 400, "Bad Request", Some(&cred_key))),
        }
        if message
            .get(attr::REQUESTED_ADDRESS_FAMILY)
            .is_some_and(|v| v.first() != Some(&0x01))
        {
            return Some(error(
                message,
                440,
                "Address Family not Supported",
                Some(&cred_key),
            ));
        }
        let client_ip = client_addr.ip();
        let per_ip = t
            .allocations
            .values()
            .filter(|a| a.client_addr.ip() == client_ip)
            .count();
        let per_lease = t.allocations.values().filter(|a| a.peer == peer).count();
        if t.allocations.len() >= MAX_ALLOCATIONS
            || per_ip >= MAX_ALLOCATIONS_PER_IP
            || per_lease >= MAX_ALLOCATIONS_PER_LEASE
        {
            return Some(error(
                message,
                486,
                "Allocation Quota Reached",
                Some(&cred_key),
            ));
        }
        let lifetime = requested_lifetime(message);
        let relay_addr = loop {
            t.next_relay = t.next_relay.wrapping_add(1);
            let n = t.next_relay;
            let addr = SocketAddrV4::new(
                Ipv4Addr::new(100, 100, (n >> 8) as u8, n as u8),
                49152 + ((n >> 16) as u16 & 0x3FFF),
            );
            if n as u8 != 0 && n as u8 != 255 && !t.by_relay.contains_key(&addr) {
                break addr;
            }
        };
        t.by_relay.insert(relay_addr, key);
        t.allocations.insert(
            key,
            Allocation {
                client_addr,
                relay: relay_addr,
                username,
                peer,
                lifetime,
                expires: Instant::now() + Duration::from_secs(lifetime as u64),
                permissions: HashMap::new(),
                channels: HashMap::new(),
                sink: sink.clone(),
                allocate_txid: message.txid,
            },
        );
        Some(allocate_success(
            message.txid,
            relay_addr,
            client_addr,
            lifetime,
            &cred_key,
        ))
    }

    /// Looks up the allocation's username, then authenticates against it.
    fn authenticated_allocation(
        &self,
        key: ClientKey,
        message: &Message,
    ) -> Result<[u8; 16], Vec<u8>> {
        let username = self
            .tables
            .lock()
            .allocations
            .get(&key)
            .map(|a| a.username.clone());
        let Some(username) = username else {
            return Err(match self.authenticate(message, None) {
                Ok((_, cred_key, _)) => error(message, 437, "Allocation Mismatch", Some(&cred_key)),
                Err(response) => response,
            });
        };
        self.authenticate(message, Some(&username))
            .map(|(_, cred_key, _)| cred_key)
    }

    fn refresh(&self, key: ClientKey, message: &Message) -> Vec<u8> {
        let cred_key = match self.authenticated_allocation(key, message) {
            Ok(k) => k,
            Err(response) => return response,
        };
        let lifetime = match message.u32_attr(attr::LIFETIME) {
            Some(0) => 0,
            _ => requested_lifetime(message),
        };
        let mut t = self.tables.lock();
        if lifetime == 0 {
            t.remove_allocation(&key);
        } else if let Some(allocation) = t.allocations.get_mut(&key) {
            allocation.lifetime = lifetime;
            allocation.expires = Instant::now() + Duration::from_secs(lifetime as u64);
        }
        let mut b = Builder::new(method::REFRESH, Class::Success, message.txid);
        b.u32_attr(attr::LIFETIME, lifetime);
        b.attr(attr::SOFTWARE, SOFTWARE);
        b.integrity(&cred_key);
        b.fingerprint();
        b.finish()
    }

    fn create_permission(&self, key: ClientKey, message: &Message) -> Vec<u8> {
        let cred_key = match self.authenticated_allocation(key, message) {
            Ok(k) => k,
            Err(response) => return response,
        };
        let peers: Vec<SocketAddr> = message
            .all(attr::XOR_PEER_ADDRESS)
            .filter_map(|v| stun::decode_xor_address(v, &message.txid))
            .collect();
        if peers.is_empty() || peers.len() > 8 {
            return error(message, 400, "Bad Request", Some(&cred_key));
        }
        let mut t = self.tables.lock();
        let Some(allocation) = t.allocations.get_mut(&key) else {
            return error(message, 437, "Allocation Mismatch", Some(&cred_key));
        };
        if peers
            .iter()
            .any(|p| p.ip() != IpAddr::V4(*allocation.peer.ip()))
        {
            return error(message, 403, "Forbidden", Some(&cred_key));
        }
        let until = Instant::now() + PERMISSION_LIFETIME;
        allocation.permissions.insert(*allocation.peer.ip(), until);
        success(message, &cred_key)
    }

    fn channel_bind(&self, key: ClientKey, message: &Message) -> Vec<u8> {
        let cred_key = match self.authenticated_allocation(key, message) {
            Ok(k) => k,
            Err(response) => return response,
        };
        let channel = message
            .get(attr::CHANNEL_NUMBER)
            .filter(|v| v.len() == 4)
            .map(|v| u16::from_be_bytes([v[0], v[1]]));
        let peer = message.xor_address(attr::XOR_PEER_ADDRESS);
        let (Some(channel), Some(peer)) = (channel, peer) else {
            return error(message, 400, "Bad Request", Some(&cred_key));
        };
        if !(0x4000..=0x7FFE).contains(&channel) {
            return error(message, 400, "Bad Request", Some(&cred_key));
        }
        let mut t = self.tables.lock();
        let Some(allocation) = t.allocations.get_mut(&key) else {
            return error(message, 437, "Allocation Mismatch", Some(&cred_key));
        };
        let SocketAddr::V4(peer) = peer else {
            return error(message, 403, "Forbidden", Some(&cred_key));
        };
        if peer != allocation.peer {
            return error(message, 403, "Forbidden", Some(&cred_key));
        }
        let now = Instant::now();
        let conflict = allocation
            .channels
            .iter()
            .any(|(n, (p, until))| *until > now && ((*n == channel) != (*p == peer)));
        if conflict {
            return error(message, 400, "Bad Request", Some(&cred_key));
        }
        allocation
            .channels
            .insert(channel, (peer, now + CHANNEL_LIFETIME));
        allocation
            .permissions
            .insert(*peer.ip(), now + PERMISSION_LIFETIME);
        success(message, &cred_key)
    }
}

fn requested_lifetime(message: &Message) -> u32 {
    message
        .u32_attr(attr::LIFETIME)
        .unwrap_or(DEFAULT_LIFETIME)
        .clamp(60, MAX_LIFETIME)
}

fn unknown_required(message: &Message) -> Vec<u16> {
    message
        .attrs
        .iter()
        .filter(|a| a.kind < 0x8000 && !KNOWN_REQUIRED.contains(&a.kind))
        .map(|a| a.kind)
        .collect()
}

fn allocate_success(
    txid: [u8; 12],
    relay: SocketAddrV4,
    client_addr: SocketAddr,
    lifetime: u32,
    key: &[u8],
) -> Vec<u8> {
    let mut b = Builder::new(method::ALLOCATE, Class::Success, txid);
    b.xor_address(attr::XOR_RELAYED_ADDRESS, SocketAddr::V4(relay));
    b.u32_attr(attr::LIFETIME, lifetime);
    b.xor_address(attr::XOR_MAPPED_ADDRESS, client_addr);
    b.attr(attr::SOFTWARE, SOFTWARE);
    b.integrity(key);
    b.fingerprint();
    b.finish()
}

fn success(message: &Message, key: &[u8]) -> Vec<u8> {
    let mut b = Builder::new(message.method, Class::Success, message.txid);
    b.attr(attr::SOFTWARE, SOFTWARE);
    b.integrity(key);
    b.fingerprint();
    b.finish()
}

fn error(message: &Message, code: u16, reason: &str, key: Option<&[u8]>) -> Vec<u8> {
    let mut b = Builder::new(message.method, Class::Error, message.txid);
    b.error(code, reason);
    b.attr(attr::SOFTWARE, SOFTWARE);
    if let Some(key) = key {
        b.integrity(key);
    }
    b.fingerprint();
    b.finish()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
