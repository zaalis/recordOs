//! Remote Access: an outbound, authenticated connection from this PC to a Vision-Pro relay
//! (agent/crates/vp-relay, docs/RELAY.md).
//!
//! - The PC proves its identity by signing the relay's challenge with the agent key.
//! - iPads reach the PC through `wss://relay/v1/connect/<agentId>`; the relay splices that
//!   socket to a data connection this PC opens with a one-time ticket. The signaling handshake
//!   and every later message remain end-to-end encrypted and pinned to the agent key.
//! - Media uses TURN on the iPad side and a tunnel (UDP, WebSocket fallback) on this side. The
//!   PC exposes one virtual ICE candidate per session; DTLS-SRTP protects the payload.

use crate::agent::Core;
use crate::security::{Severity, Verdict};
use crate::signaling::{self, Route, SignalTransport};
use crate::util::{b64u, from_b64u, now_ms};
use futures_util::{SinkExt, StreamExt};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{timeout, Instant};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

pub const LABEL_REGISTER: &str = "Vision-Pro/1 relay-register";
const KEEPALIVE: Duration = Duration::from_secs(15);
const UDP_STALE: Duration = Duration::from_secs(45);
const CONTROL_IDLE: Duration = Duration::from_secs(90);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Tunnel frames, mirror of vp-relay `wire`.
mod wire {
    use std::net::{Ipv4Addr, SocketAddrV4};

    pub const HELLO: u8 = 0xA1;
    pub const ACK: u8 = 0xA2;
    pub const DATA: u8 = 0xA3;
    pub const KEEPALIVE: u8 = 0xA4;
    pub const HEADER: usize = 13;

    pub fn encode(peer: SocketAddrV4, relay: SocketAddrV4, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER + payload.len());
        out.push(DATA);
        out.extend_from_slice(&peer.ip().octets());
        out.extend_from_slice(&peer.port().to_be_bytes());
        out.extend_from_slice(&relay.ip().octets());
        out.extend_from_slice(&relay.port().to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    pub fn decode(frame: &[u8]) -> Option<(SocketAddrV4, SocketAddrV4, &[u8])> {
        if frame.len() <= HEADER || frame[0] != DATA {
            return None;
        }
        let addr = |b: &[u8]| {
            SocketAddrV4::new(
                Ipv4Addr::new(b[0], b[1], b[2], b[3]),
                u16::from_be_bytes([b[4], b[5]]),
            )
        };
        Some((addr(&frame[1..7]), addr(&frame[7..13]), &frame[HEADER..]))
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RelayState {
    Disabled,
    Connecting,
    Connected,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayStatus {
    pub state: RelayState,
    pub url: Option<String>,
    pub error: Option<String>,
    pub connected_at: Option<u64>,
    /// Media reaches the relay over UDP (otherwise over the WebSocket fallback).
    pub udp: bool,
}

#[derive(Debug, Clone)]
pub struct TurnCredential {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
    pub peer: SocketAddrV4,
}

pub type Datagram = (SocketAddr, Vec<u8>);

/// Media path of one relayed session, consumed by the WebRTC task.
pub struct RelayMediaLink {
    pub incoming: mpsc::Receiver<Datagram>,
    pub sender: RelaySender,
}

#[derive(Clone)]
pub struct RelaySender {
    local: SocketAddrV4,
    hub: Arc<RelayHub>,
}

impl RelaySender {
    /// The virtual ICE candidate address of this session.
    pub fn local(&self) -> SocketAddr {
        SocketAddr::V4(self.local)
    }

    pub fn send(&self, destination: SocketAddr, payload: &[u8]) {
        if let SocketAddr::V4(destination) = destination {
            self.hub
                .send_frame(wire::encode(self.local, destination, payload));
        }
    }
}

enum Control {
    Text(String),
    Binary(Vec<u8>),
}

struct Link {
    control: mpsc::Sender<Control>,
    udp: Option<(Arc<UdpSocket>, SocketAddr)>,
}

pub struct RelayHub {
    status: RwLock<RelayStatus>,
    link: RwLock<Option<Link>>,
    udp_bound: AtomicBool,
    pending: Mutex<HashMap<u32, oneshot::Sender<Result<TurnCredential, String>>>>,
    next_id: AtomicU32,
    routes: RwLock<HashMap<SocketAddrV4, mpsc::Sender<Datagram>>>,
    /// Incremented for every `run`; a stopped run can no longer touch the state of its successor.
    generation: AtomicU64,
}

impl Default for RelayHub {
    fn default() -> Self {
        RelayHub {
            status: RwLock::new(RelayStatus {
                state: RelayState::Disabled,
                url: None,
                error: None,
                connected_at: None,
                udp: false,
            }),
            link: RwLock::new(None),
            udp_bound: AtomicBool::new(false),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU32::new(1),
            routes: RwLock::new(HashMap::new()),
            generation: AtomicU64::new(0),
        }
    }
}

impl RelayHub {
    pub fn status(&self) -> RelayStatus {
        let mut status = self.status.read().clone();
        status.udp =
            status.state == RelayState::Connected && self.udp_bound.load(Ordering::Relaxed);
        status
    }

    /// Relay address shared with authenticated iPads, only while this PC is reachable through it.
    pub fn public_url(&self) -> Option<String> {
        let status = self.status.read();
        (status.state == RelayState::Connected)
            .then(|| status.url.clone())
            .flatten()
    }

    fn current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }

    fn set_status(
        &self,
        core: &Core,
        generation: u64,
        state: RelayState,
        url: Option<&str>,
        error: Option<String>,
    ) {
        {
            let mut status = self.status.write();
            if !self.current(generation) {
                return;
            }
            if status.state == state && status.error == error {
                return;
            }
            status.connected_at = (state == RelayState::Connected).then(now_ms);
            status.state = state;
            status.url = url.map(str::to_string);
            status.error = error;
        }
        let _ = core.events.send(crate::events::AgentEvent::RelayChanged);
    }

    /// Asks the relay for a TURN credential bound to a fresh virtual peer address.
    pub async fn issue(&self, ttl_secs: u64) -> anyhow::Result<TurnCredential> {
        let control = self
            .link
            .read()
            .as_ref()
            .map(|l| l.control.clone())
            .ok_or_else(|| anyhow::anyhow!("relay is not connected"))?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        let request = json!({ "t": "turn.issue", "id": id, "ttl": ttl_secs });
        if control
            .send(Control::Text(request.to_string()))
            .await
            .is_err()
        {
            self.pending.lock().remove(&id);
            anyhow::bail!("relay is not connected");
        }
        match timeout(Duration::from_secs(8), rx).await {
            Ok(Ok(Ok(credential))) => Ok(credential),
            Ok(Ok(Err(code))) => anyhow::bail!("relay refused a media credential ({code})"),
            _ => {
                self.pending.lock().remove(&id);
                anyhow::bail!("relay did not answer")
            }
        }
    }

    pub fn open_route(self: &Arc<Self>, peer: SocketAddrV4) -> RelayMediaLink {
        let (tx, incoming) = mpsc::channel(1024);
        self.routes.write().insert(peer, tx);
        RelayMediaLink {
            incoming,
            sender: RelaySender {
                local: peer,
                hub: self.clone(),
            },
        }
    }

    pub fn close_route(&self, peer: SocketAddrV4) {
        self.routes.write().remove(&peer);
        if let Some(link) = self.link.read().as_ref() {
            let release = json!({ "t": "turn.release", "peer": peer.to_string() });
            let _ = link.control.try_send(Control::Text(release.to_string()));
        }
    }

    fn send_frame(&self, frame: Vec<u8>) {
        let link = self.link.read();
        let Some(link) = link.as_ref() else { return };
        match &link.udp {
            Some((socket, relay)) if self.udp_bound.load(Ordering::Relaxed) => {
                let _ = socket.try_send_to(&frame, *relay);
            }
            _ => {
                let _ = link.control.try_send(Control::Binary(frame));
            }
        }
    }

    fn deliver(&self, frame: &[u8]) {
        if let Some((peer, relay, payload)) = wire::decode(frame) {
            if let Some(route) = self.routes.read().get(&peer) {
                let _ = route.try_send((SocketAddr::V4(relay), payload.to_vec()));
            }
        }
    }

    fn resolve(&self, id: u32, result: Result<TurnCredential, String>) {
        if let Some(tx) = self.pending.lock().remove(&id) {
            let _ = tx.send(result);
        }
    }

    fn attach(&self, generation: u64, link: Link) {
        let mut slot = self.link.write();
        if self.current(generation) {
            *slot = Some(link);
        }
    }

    fn disconnect(&self, generation: u64) {
        let mut slot = self.link.write();
        if !self.current(generation) {
            return;
        }
        *slot = None;
        drop(slot);
        self.udp_bound.store(false, Ordering::Relaxed);
        for (_, tx) in self.pending.lock().drain() {
            let _ = tx.send(Err("disconnected".into()));
        }
    }
}

/// Normalizes a user-entered relay address to `wss://host[:port]`. Plain `ws://` is accepted
/// only for loopback addresses (local testing).
pub fn normalize_url(input: &str) -> Option<String> {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed.len() > 255 {
        return None;
    }
    let (secure, rest) = if let Some(rest) = trimmed
        .strip_prefix("wss://")
        .or_else(|| trimmed.strip_prefix("https://"))
    {
        (true, rest)
    } else if let Some(rest) = trimmed
        .strip_prefix("ws://")
        .or_else(|| trimmed.strip_prefix("http://"))
    {
        (false, rest)
    } else if trimmed.contains("://") {
        return None;
    } else {
        (true, trimmed)
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let valid = !authority.is_empty()
        && authority
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'));
    if !valid {
        return None;
    }
    let host = host_of(authority);
    if !(secure || host == "localhost" || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())) {
        return None;
    }
    Some(format!(
        "{}://{}",
        if secure { "wss" } else { "ws" },
        authority.to_ascii_lowercase()
    ))
}

fn host_of(authority: &str) -> &str {
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split(']').next().unwrap_or_default();
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => authority,
    }
}

fn tls_config() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            let native = rustls_native_certs::load_native_certs();
            for certificate in native.certs {
                let _ = roots.add(certificate);
            }
            let config = rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .expect("TLS 1.2/1.3 are supported")
            .with_root_certificates(roots)
            .with_no_client_auth();
            Arc::new(config)
        })
        .clone()
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn connect(url: &str) -> anyhow::Result<Socket> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(1 << 20))
        .max_frame_size(Some(1 << 20));
    let connecting = tokio_tungstenite::connect_async_tls_with_config(
        url,
        Some(config),
        true,
        Some(Connector::Rustls(tls_config())),
    );
    let (socket, _) = timeout(Duration::from_secs(10), connecting)
        .await
        .map_err(|_| anyhow::anyhow!("délai dépassé"))??;
    Ok(socket)
}

fn describe_connect_error(error: &anyhow::Error) -> String {
    let text = error.to_string();
    if text.contains("certificate") || text.contains("InvalidCertificate") {
        "Le certificat du relais n’est pas valide.".into()
    } else if text.contains("délai") {
        "Le relais ne répond pas.".into()
    } else if text.contains("404") || text.contains("HTTP error") {
        "Cette adresse n’est pas un relais Vision-Pro.".into()
    } else {
        "Impossible de joindre le relais.".into()
    }
}

/// Keeps this PC registered on the relay until `shutdown` fires, reconnecting with backoff.
pub async fn run(
    core: Arc<Core>,
    url: String,
    token: Option<String>,
    mut shutdown: watch::Receiver<bool>,
) {
    let hub = core.relay.clone();
    let generation = hub.generation.fetch_add(1, Ordering::SeqCst) + 1;
    hub.set_status(&core, generation, RelayState::Connecting, Some(&url), None);
    let mut backoff = Duration::from_secs(1);
    loop {
        let started = Instant::now();
        let session_shutdown = shutdown.clone();
        let outcome = tokio::select! {
            _ = shutdown.changed() => break,
            outcome = session(&core, generation, &url, token.as_deref(), session_shutdown) => outcome,
        };
        let was_connected = hub.status.read().state == RelayState::Connected;
        hub.disconnect(generation);
        let message = outcome
            .err()
            .unwrap_or_else(|| "La connexion au relais a été interrompue.".to_string());
        if was_connected {
            core.audit.record(
                Severity::Notice,
                "relay.disconnected",
                format!("Remote Access relay disconnected: {message}"),
            );
        }
        // Stay in the error state while retrying so the UI does not flicker.
        hub.set_status(
            &core,
            generation,
            RelayState::Error,
            Some(&url),
            Some(message),
        );
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
    hub.disconnect(generation);
    hub.set_status(&core, generation, RelayState::Disabled, None, None);
}

async fn next_text(
    stream: &mut futures_util::stream::SplitStream<Socket>,
    wait: Duration,
) -> Result<String, String> {
    loop {
        match timeout(wait, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => return Ok(text.to_string()),
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
            Ok(_) => return Err("La connexion au relais a été fermée.".into()),
            Err(_) => return Err("Le relais ne répond pas.".into()),
        }
    }
}

async fn session(
    core: &Arc<Core>,
    generation: u64,
    url: &str,
    token: Option<&str>,
    shutdown: watch::Receiver<bool>,
) -> Result<(), String> {
    let socket = connect(&format!("{url}/v1/agent"))
        .await
        .map_err(|e| describe_connect_error(&e))?;
    let (mut sink, mut stream) = socket.split();

    let challenge: Value =
        serde_json::from_str(&next_text(&mut stream, Duration::from_secs(10)).await?)
            .map_err(|_| "Réponse du relais invalide.".to_string())?;
    let nonce = challenge["nonce"]
        .as_str()
        .and_then(|n| from_b64u(n, Some(32)))
        .filter(|_| challenge["t"] == "challenge")
        .ok_or_else(|| "Cette adresse n’est pas un relais Vision-Pro compatible.".to_string())?;
    let identity = core.identity.read().clone();
    let mut register = json!({
        "t": "register",
        "v": 1,
        "agentPub": b64u(identity.public_raw()),
        "sig": b64u(&identity.sign(LABEL_REGISTER, &nonce)),
    });
    if let Some(token) = token {
        register["token"] = json!(token);
    }
    sink.send(Message::Text(register.to_string().into()))
        .await
        .map_err(|_| "La connexion au relais a été fermée.".to_string())?;
    let reply: Value =
        serde_json::from_str(&next_text(&mut stream, Duration::from_secs(10)).await?)
            .map_err(|_| "Réponse du relais invalide.".to_string())?;
    if reply["t"] != "registered" {
        return Err(match reply["code"].as_str() {
            Some("forbidden") => {
                "Le relais a refusé ce PC : jeton d’accès manquant ou incorrect.".into()
            }
            Some("version") => "Ce relais utilise une version incompatible.".into(),
            Some("capacity") => "Le relais est complet.".into(),
            _ => "Le relais a refusé l’enregistrement de ce PC.".into(),
        });
    }
    if reply["agentId"].as_str() != Some(identity.agent_id.as_str()) {
        return Err("Le relais a renvoyé une identité inattendue.".into());
    }

    let (control_tx, mut control_rx) = mpsc::channel::<Control>(4096);
    let udp = start_udp(url, &reply).await;
    let udp_shutdown = shutdown.clone();
    let udp_task = udp.as_ref().map(|(socket, relay_addr, token)| {
        tokio::spawn(udp_loop(
            core.relay.clone(),
            generation,
            socket.clone(),
            *relay_addr,
            *token,
            udp_shutdown,
        ))
    });
    core.relay.attach(
        generation,
        Link {
            control: control_tx,
            udp: udp.map(|(socket, addr, _)| (socket, addr)),
        },
    );
    core.relay
        .set_status(core, generation, RelayState::Connected, Some(url), None);
    core.audit.record(
        Severity::Notice,
        "relay.connected",
        "This PC is reachable through the Remote Access relay.",
    );

    let result = loop {
        tokio::select! {
            outgoing = control_rx.recv() => {
                let message = match outgoing {
                    Some(Control::Text(text)) => Message::Text(text.into()),
                    Some(Control::Binary(frame)) => Message::Binary(frame.into()),
                    None => break Ok(()),
                };
                if sink.send(message).await.is_err() {
                    break Ok(());
                }
            }
            incoming = timeout(CONTROL_IDLE, stream.next()) => {
                match incoming {
                    Ok(Some(Ok(Message::Text(text)))) => {
                        let Ok(message) = serde_json::from_str::<Value>(&text) else { continue };
                        match message["t"].as_str() {
                            Some("incoming") => {
                                if let Some(ticket) = message["ticket"].as_str().filter(|t| t.len() == 32 && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')) {
                                    let ip = message["ip"].as_str().and_then(|ip| ip.parse().ok()).unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
                                    tokio::spawn(accept(core.clone(), format!("{url}/v1/accept/{ticket}"), ip, shutdown.clone()));
                                }
                            }
                            Some("turn.credential") => {
                                let id = message["id"].as_u64().unwrap_or(0) as u32;
                                core.relay.resolve(id, parse_credential(&message).ok_or_else(|| "invalid".to_string()));
                            }
                            Some("turn.error") => {
                                let id = message["id"].as_u64().unwrap_or(0) as u32;
                                core.relay.resolve(id, Err(message["code"].as_str().unwrap_or("error").to_string()));
                            }
                            Some("error") => break Err("Le relais a fermé la connexion.".to_string()),
                            _ => {}
                        }
                    }
                    Ok(Some(Ok(Message::Binary(frame)))) => core.relay.deliver(&frame),
                    Ok(Some(Ok(Message::Ping(payload)))) => {
                        let _ = sink.send(Message::Pong(payload)).await;
                    }
                    Ok(Some(Ok(Message::Pong(_)))) => {}
                    Ok(Some(Ok(_))) | Ok(Some(Err(_))) | Ok(None) => break Ok(()),
                    Err(_) => break Err("Le relais ne répond plus.".to_string()),
                }
            }
        }
    };
    if let Some(task) = udp_task {
        task.abort();
    }
    let _ = sink.send(Message::Close(None)).await;
    result
}

fn parse_credential(message: &Value) -> Option<TurnCredential> {
    let urls: Vec<String> = message["urls"]
        .as_array()?
        .iter()
        .filter_map(|u| u.as_str())
        .filter(|u| u.starts_with("turn:") || u.starts_with("turns:"))
        .take(4)
        .map(str::to_string)
        .collect();
    (!urls.is_empty()).then_some(())?;
    Some(TurnCredential {
        urls,
        username: message["username"].as_str()?.to_string(),
        credential: message["credential"].as_str()?.to_string(),
        peer: message["peer"].as_str()?.parse().ok()?,
    })
}

async fn start_udp(url: &str, reply: &Value) -> Option<(Arc<UdpSocket>, SocketAddr, [u8; 32])> {
    let token: [u8; 32] = from_b64u(reply["udpToken"].as_str()?, Some(32))?
        .try_into()
        .ok()?;
    let port = u16::try_from(reply["udpPort"].as_u64()?).ok()?;
    let authority = url.split("://").nth(1)?;
    let host = host_of(authority);
    let relay_addr = tokio::net::lookup_host((host, port))
        .await
        .ok()?
        .find(SocketAddr::is_ipv4)?;
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await.ok()?;
    Some((Arc::new(socket), relay_addr, token))
}

async fn udp_loop(
    hub: Arc<RelayHub>,
    generation: u64,
    socket: Arc<UdpSocket>,
    relay: SocketAddr,
    token: [u8; 32],
    shutdown: watch::Receiver<bool>,
) {
    let hello = {
        let mut frame = vec![wire::HELLO];
        frame.extend_from_slice(&token);
        frame
    };
    let keepalive = {
        let mut frame = vec![wire::KEEPALIVE];
        frame.extend_from_slice(&token);
        frame
    };
    let _ = socket.send_to(&hello, relay).await;
    let mut last_ack: Option<Instant> = None;
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    let mut last_keepalive = Instant::now();
    let mut buf = vec![0u8; 65_536];
    loop {
        tokio::select! {
            _ = wait_for_shutdown(shutdown.clone()) => break,
            _ = tick.tick() => {
                let bound = last_ack.is_some_and(|at| at.elapsed() < UDP_STALE);
                if !hub.current(generation) {
                    break;
                }
                hub.udp_bound.store(bound, Ordering::Relaxed);
                // Retry the hello quickly until acknowledged, then keep the NAT mapping alive.
                if !bound || last_keepalive.elapsed() >= KEEPALIVE {
                    let frame = if last_ack.is_none() { &hello } else { &keepalive };
                    let _ = socket.send_to(frame, relay).await;
                    last_keepalive = Instant::now();
                }
            }
            received = socket.recv_from(&mut buf) => match received {
                Ok((n, from)) if from == relay && n > 0 => {
                    if buf[0] == wire::ACK {
                        last_ack = Some(Instant::now());
                        if hub.current(generation) {
                            hub.udp_bound.store(true, Ordering::Relaxed);
                        }
                    } else {
                        hub.deliver(&buf[..n]);
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }
}

async fn accept(core: Arc<Core>, url: String, ip: IpAddr, shutdown: watch::Receiver<bool>) {
    if core.settings.read().network.mode != crate::config::NetworkMode::RemoteAccess {
        return;
    }
    if let Verdict::LockedOut(_) | Verdict::RateLimited(_) = core.guard.check_connection(ip) {
        return;
    }
    let socket = tokio::select! {
        _ = wait_for_shutdown(shutdown.clone()) => return,
        result = connect(&url) => result,
    };
    match socket {
        Ok(socket) => {
            signaling::run(
                core,
                TungsteniteTransport(socket),
                ip,
                Route::Relay,
                None,
                shutdown,
            )
            .await
        }
        Err(error) => tracing::debug!(%error, "relay accept failed"),
    }
}

async fn wait_for_shutdown(mut shutdown: watch::Receiver<bool>) {
    if *shutdown.borrow() {
        return;
    }
    let _ = shutdown.changed().await;
}

struct TungsteniteTransport(Socket);

impl SignalTransport for TungsteniteTransport {
    async fn send_text(&mut self, text: String) -> anyhow::Result<()> {
        self.0.send(Message::Text(text.into())).await?;
        Ok(())
    }

    async fn recv_text(&mut self) -> Option<anyhow::Result<String>> {
        loop {
            match self.0.next().await? {
                Ok(Message::Text(text)) => {
                    if text.len() > 96 * 1024 {
                        return Some(Err(anyhow::anyhow!("oversized signaling message")));
                    }
                    return Some(Ok(text.to_string()));
                }
                Ok(Message::Binary(_)) => {
                    return Some(Err(anyhow::anyhow!("binary signaling not allowed")))
                }
                Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => continue,
                Ok(Message::Close(_)) => return None,
                Err(error) => return Some(Err(error.into())),
            }
        }
    }

    async fn close(&mut self) {
        let _ = self.0.close(None).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_urls_are_normalized_and_restricted() {
        assert_eq!(
            normalize_url("relay.example.com").as_deref(),
            Some("wss://relay.example.com")
        );
        assert_eq!(
            normalize_url("https://Relay.Example.com/").as_deref(),
            Some("wss://relay.example.com")
        );
        assert_eq!(
            normalize_url("wss://relay.example.com:8443/v1").as_deref(),
            Some("wss://relay.example.com:8443")
        );
        assert_eq!(
            normalize_url("ws://127.0.0.1:8080").as_deref(),
            Some("ws://127.0.0.1:8080")
        );
        assert_eq!(normalize_url("ws://relay.example.com"), None);
        assert_eq!(normalize_url("ftp://relay.example.com"), None);
        assert_eq!(normalize_url("wss://evil.com@relay"), None);
        assert_eq!(normalize_url("   "), None);
        assert_eq!(host_of("[::1]:8080"), "::1");
        assert_eq!(host_of("relay.example.com:443"), "relay.example.com");
    }

    #[test]
    fn tunnel_frames_match_the_relay_format() {
        let peer: SocketAddrV4 = "100.64.0.9:40009".parse().unwrap();
        let relay: SocketAddrV4 = "100.100.1.2:50001".parse().unwrap();
        let frame = wire::encode(peer, relay, b"x");
        assert_eq!(frame[..7], [0xA3, 100, 64, 0, 9, 0x9c, 0x49]);
        assert_eq!(wire::decode(&frame), Some((peer, relay, &b"x"[..])));
    }
}
