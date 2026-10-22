//! Remote Access: an outbound, authenticated connection from this PC to a Vision-Pro relay
//! (agent/crates/vp-relay, docs/RELAY.md).
//!
//! - The PC proves its identity by signing the relay's challenge with the agent key.
//! - iPads reach the PC through `wss://relay/v1/connect/<agentId>`; the relay splices that
//!   socket to a data connection this PC opens with a one-time ticket. The handshake and every
//!   later message (input included) remain end-to-end encrypted and pinned to the agent key.

use crate::agent::Core;
use crate::security::{Severity, Verdict};
use crate::signaling::{self, Route, SignalTransport};
use crate::util::{b64u, from_b64u, now_ms};
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use serde::Serialize;
use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::{timeout, Instant};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

pub const LABEL_REGISTER: &str = "Vision-Pro/1 relay-register";
const CONTROL_IDLE: Duration = Duration::from_secs(90);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

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
}

pub struct RelayHub {
    status: RwLock<RelayStatus>,
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
            }),
            generation: AtomicU64::new(0),
        }
    }
}

impl RelayHub {
    pub fn status(&self) -> RelayStatus {
        self.status.read().clone()
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

    core.relay
        .set_status(core, generation, RelayState::Connected, Some(url), None);
    core.audit.record(
        Severity::Notice,
        "relay.connected",
        "This PC is reachable through the Remote Access relay.",
    );

    let result = loop {
        tokio::select! {
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
                            Some("error") => break Err("Le relais a fermé la connexion.".to_string()),
                            _ => {}
                        }
                    }
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
    let _ = sink.send(Message::Close(None)).await;
    result
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
}
