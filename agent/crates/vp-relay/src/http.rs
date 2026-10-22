//! HTTP(S) endpoints:
//!   GET /healthz                 liveness
//!   GET /v1/agent                agent control WebSocket (challenge/response registration)
//!   GET /v1/connect/{agentId}    iPad signaling WebSocket, spliced to the agent
//!   GET /v1/accept/{ticket}      agent data WebSocket for one iPad (one-time ticket)
//!
//! The relay never sees signaling plaintext: after the plaintext handshake (whose keys are
//! pinned by the iPad), every message is AES-GCM encrypted between the iPad and the PC.

use crate::hub::Outgoing;
use crate::util::{agent_id_from_public_key, b64u, from_b64u, is_agent_id, random_bytes};
use crate::{wire, Relay};
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, Request, Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use hyper_util::rt::TokioIo;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::net::{IpAddr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::time::timeout;
use tower::ServiceExt;

pub const LABEL_REGISTER: &str = "Vision-Pro/1 relay-register";
const MAX_SIGNAL_MESSAGE: usize = 96 * 1024;
const MAX_CONTROL_MESSAGE: usize = 256 * 1024;
const AGENT_IDLE: Duration = Duration::from_secs(75);
const PING_EVERY: Duration = Duration::from_secs(20);
const MAX_SIGNAL_RATE: u32 = 400;

pub fn router(relay: Arc<Relay>) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/agent", get(agent_handler))
        .route("/v1/pair", post(pair_handler))
        .route("/v1/connect/{agent_id}", get(connect_handler))
        .route("/v1/accept/{ticket}", get(accept_handler))
        .with_state(relay)
}

pub async fn serve(
    relay: Arc<Relay>,
    listener: TcpListener,
    tls: Option<tokio_rustls::TlsAcceptor>,
    mut shutdown: watch::Receiver<bool>,
) {
    let router = router(relay);
    loop {
        let (stream, peer) = tokio::select! {
            _ = shutdown.changed() => break,
            accepted = listener.accept() => match accepted {
                Ok(pair) => pair,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            }
        };
        let _ = stream.set_nodelay(true);
        let router = router.clone();
        match tls.clone() {
            Some(acceptor) => {
                tokio::spawn(async move {
                    if let Ok(Ok(stream)) =
                        timeout(Duration::from_secs(10), acceptor.accept(stream)).await
                    {
                        serve_connection(TokioIo::new(stream), peer, router).await;
                    }
                });
            }
            None => {
                tokio::spawn(serve_connection(TokioIo::new(stream), peer, router));
            }
        }
    }
}

async fn serve_connection<I>(io: TokioIo<I>, peer: SocketAddr, router: Router)
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = hyper::service::service_fn(move |mut request: Request<hyper::body::Incoming>| {
        request.extensions_mut().insert(ConnectInfo(peer));
        let router = router.clone();
        async move { router.oneshot(request.map(Body::new)).await }
    });
    let _ = hyper::server::conn::http1::Builder::new()
        .timer(hyper_util::rt::TokioTimer::new())
        .header_read_timeout(Duration::from_secs(10))
        .serve_connection(io, service)
        .with_upgrades()
        .await;
}

fn client_ip(relay: &Relay, peer: SocketAddr, headers: &HeaderMap) -> IpAddr {
    if relay.config.trust_proxy {
        let forwarded = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit(',').next())
            .and_then(|v| v.trim().parse::<IpAddr>().ok());
        if let Some(ip) = forwarded {
            return ip;
        }
    }
    peer.ip()
}

// --- Agent control ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Register {
    t: String,
    v: u32,
    agent_pub: String,
    sig: String,
    #[serde(default)]
    token: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "t")]
enum AgentMessage {
    #[serde(rename = "pair.publish")]
    PairPublish { code: String, expires: u64 },
    #[serde(rename = "turn.issue")]
    TurnIssue { id: u32, ttl: u64 },
    #[serde(rename = "turn.release")]
    TurnRelease { peer: String },
}

async fn agent_handler(
    ws: WebSocketUpgrade,
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response<Body> {
    let ip = client_ip(&relay, peer, &headers);
    if !relay.register_limiter.allow(ip) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    ws.max_message_size(MAX_CONTROL_MESSAGE)
        .max_frame_size(MAX_CONTROL_MESSAGE)
        .on_upgrade(move |socket| agent_session(relay, ip, socket))
}

fn verify_registration(
    register: &Register,
    nonce: &[u8],
    required_token: Option<&str>,
) -> Result<String, &'static str> {
    if register.t != "register" {
        return Err("protocol");
    }
    if register.v != 1 {
        return Err("version");
    }
    let public_raw = from_b64u(&register.agent_pub, 65).ok_or("protocol")?;
    let signature = from_b64u(&register.sig, 64).ok_or("protocol")?;
    if public_raw[0] != 0x04 {
        return Err("protocol");
    }
    let key = VerifyingKey::from_sec1_bytes(&public_raw).map_err(|_| "protocol")?;
    let signature = Signature::from_slice(&signature).map_err(|_| "bad_signature")?;
    let signature = signature.normalize_s().unwrap_or(signature);
    let mut message = LABEL_REGISTER.as_bytes().to_vec();
    message.extend_from_slice(nonce);
    key.verify(&message, &signature)
        .map_err(|_| "bad_signature")?;
    if let Some(required) = required_token {
        let presented = register.token.as_deref().unwrap_or("");
        let matches: bool = required.as_bytes().ct_eq(presented.as_bytes()).into();
        if !matches {
            return Err("forbidden");
        }
    }
    Ok(agent_id_from_public_key(&public_raw))
}

async fn agent_session(relay: Arc<Relay>, ip: IpAddr, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let nonce: [u8; 32] = random_bytes();
    let challenge = json!({ "t": "challenge", "v": 1, "nonce": b64u(&nonce) });
    if sink
        .send(Message::Text(challenge.to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    let register = match timeout(Duration::from_secs(10), stream.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<Register>(&text).ok(),
        _ => None,
    };
    let verdict = match &register {
        Some(register) => {
            verify_registration(register, &nonce, relay.config.access_token.as_deref())
        }
        None => Err("protocol"),
    };
    let agent_id = match verdict {
        Ok(id) => id,
        Err(code) => {
            tracing::info!(%ip, code, "agent registration refused");
            let _ = sink
                .send(Message::Text(
                    json!({ "t": "error", "code": code }).to_string().into(),
                ))
                .await;
            let _ = sink.send(Message::Close(None)).await;
            return;
        }
    };
    let Some(registration) = relay.hub.register(&agent_id) else {
        let _ = sink
            .send(Message::Text(
                json!({ "t": "error", "code": "capacity" })
                    .to_string()
                    .into(),
            ))
            .await;
        return;
    };
    let conn_id = registration.conn_id;
    let mut rx = registration.rx;
    let tx = registration.tx;
    tracing::info!(%ip, agent = %agent_id, "agent registered");
    let registered = json!({
        "t": "registered",
        "agentId": agent_id,
        "udpToken": b64u(&registration.udp_token),
        "udpPort": relay.turn_udp_port,
        "turnUrls": relay.turn_urls,
    });
    if sink
        .send(Message::Text(registered.to_string().into()))
        .await
        .is_err()
    {
        relay.hub.unregister(&agent_id, conn_id);
        return;
    }

    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    let mut last_seen = tokio::time::Instant::now();
    let mut stopping = relay.stopping.clone();
    loop {
        tokio::select! {
            _ = stopping.changed() => break,
            _ = ping.tick() => {
                if last_seen.elapsed() > AGENT_IDLE || sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            outgoing = rx.recv() => {
                let message = match outgoing {
                    Some(Outgoing::Text(text)) => Message::Text(text.into()),
                    Some(Outgoing::Binary(frame)) => Message::Binary(frame.into()),
                    Some(Outgoing::Close) | None => break,
                };
                if sink.send(message).await.is_err() {
                    break;
                }
            }
            incoming = stream.next() => {
                last_seen = tokio::time::Instant::now();
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<AgentMessage>(&text) {
                            Ok(AgentMessage::PairPublish { code, expires }) => {
                                let mut invitations = relay.invitations.lock();
                                invitations.retain(|_, (_, _, until, _)| *until > crate::util::now_unix());
                                if code.len() == 4 && code.bytes().all(|b| b.is_ascii_digit()) && expires > crate::util::now_unix() && expires <= crate::util::now_unix() + 125 {
                                    if invitations.iter().any(|(owner, (pin, _, _, _))| owner != &agent_id && pin == &code) {
                                        let _ = tx.try_send(Outgoing::Text(json!({"t":"pair.collision"}).to_string()));
                                    } else if invitations.len() < 10000 || invitations.contains_key(&agent_id) {
                                        let fingerprint = b64u(&Sha256::digest(from_b64u(&register.as_ref().unwrap().agent_pub, 65).unwrap()));
                                        let attempts = invitations.get(&agent_id).filter(|(pin, _, _, _)| pin == &code).map(|x| x.3).unwrap_or(0);
                                        invitations.insert(agent_id.clone(), (code, fingerprint, expires, attempts));
                                    }
                                } else { invitations.remove(&agent_id); }
                            }
                            Ok(AgentMessage::TurnIssue { id, ttl }) => {
                                let reply = match relay.turn.issue(&agent_id, conn_id, ttl) {
                                    Some(credential) => json!({
                                        "t": "turn.credential",
                                        "id": id,
                                        "username": credential.username,
                                        "credential": credential.password,
                                        "peer": credential.peer.to_string(),
                                        "expiresAt": credential.expires_unix * 1000,
                                        "urls": relay.turn_urls,
                                    }),
                                    None => json!({ "t": "turn.error", "id": id, "code": "quota" }),
                                };
                                let _ = tx.try_send(Outgoing::Text(reply.to_string()));
                            }
                            Ok(AgentMessage::TurnRelease { peer }) => {
                                if let Ok(peer) = peer.parse::<SocketAddrV4>() {
                                    relay.turn.release(&agent_id, peer);
                                }
                            }
                            Err(_) => {}
                        }
                    }
                    Some(Ok(Message::Binary(frame))) => {
                        if let Some((peer, relay_addr, payload)) = wire::decode_data(&frame) {
                            relay.turn.deliver_from_agent(&agent_id, conn_id, peer, relay_addr, payload);
                        }
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                    _ => break,
                }
            }
        }
    }
    relay.hub.unregister(&agent_id, conn_id);
    relay.turn.release_connection(conn_id);
    relay.invitations.lock().remove(&agent_id);
    let _ = sink.send(Message::Close(None)).await;
    tracing::info!(agent = %agent_id, "agent disconnected");
}

// --- iPad signaling ----------------------------------------------------------------------------

async fn connect_handler(
    ws: WebSocketUpgrade,
    Path(agent_id): Path<String>,
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response<Body> {
    let ip = client_ip(&relay, peer, &headers);
    if !is_agent_id(&agent_id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !relay.connect_limiter.allow(ip) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    ws.max_message_size(MAX_SIGNAL_MESSAGE)
        .max_frame_size(MAX_SIGNAL_MESSAGE)
        .on_upgrade(move |socket| client_session(relay, agent_id, ip, socket))
}

async fn client_session(relay: Arc<Relay>, agent_id: String, ip: IpAddr, mut socket: WebSocket) {
    let agent_socket = match relay.hub.request_accept(&agent_id, ip) {
        Some(rx) => timeout(Duration::from_secs(10), rx)
            .await
            .ok()
            .and_then(Result::ok),
        None => None,
    };
    let Some(agent_socket) = agent_socket else {
        let _ = socket
            .send(Message::Text(
                json!({ "t": "error", "code": "offline" })
                    .to_string()
                    .into(),
            ))
            .await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    };
    splice(socket, agent_socket, relay.stopping.clone()).await;
}

async fn accept_handler(
    ws: WebSocketUpgrade,
    Path(ticket): Path<String>,
    State(relay): State<Arc<Relay>>,
) -> Response<Body> {
    if ticket.len() != 32 {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(tx) = relay.hub.take_ticket(&ticket) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    ws.max_message_size(MAX_SIGNAL_MESSAGE)
        .max_frame_size(MAX_SIGNAL_MESSAGE)
        .on_upgrade(move |socket| async move {
            let _ = tx.send(socket);
        })
}

/// Copies text frames in both directions until either side closes or misbehaves.
async fn splice(client: WebSocket, agent: WebSocket, mut stopping: watch::Receiver<bool>) {
    let (mut client_tx, mut client_rx) = client.split();
    let (mut agent_tx, mut agent_rx) = agent.split();
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    ping.tick().await;
    let mut window = (tokio::time::Instant::now(), 0u32);
    let started = tokio::time::Instant::now();
    let close_reason: &str;
    loop {
        if window.0.elapsed() >= Duration::from_secs(1) {
            window = (tokio::time::Instant::now(), 0);
        }
        tokio::select! {
            _ = stopping.changed() => { close_reason = "server_shutdown"; break; },
            message = client_rx.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    window.1 += 1;
                    if window.1 > MAX_SIGNAL_RATE {
                        close_reason = "rate_limited";
                        tracing::warn!(frames = window.1, "relay input rate exceeded");
                        break;
                    }
                    if agent_tx.send(Message::Text(text)).await.is_err() { close_reason = "agent_write_failed"; break; }
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                _ => { close_reason = "client_closed"; break; },
            },
            message = agent_rx.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    if client_tx.send(Message::Text(text)).await.is_err() {
                        close_reason = "client_write_failed"; break;
                    }
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                _ => { close_reason = "agent_closed"; break; },
            },
            _ = ping.tick() => {
                if client_tx.send(Message::Ping(Vec::new().into())).await.is_err()
                    || agent_tx.send(Message::Ping(Vec::new().into())).await.is_err()
                {
                    close_reason = "heartbeat_write_failed"; break;
                }
            }
        }
    }
    tracing::info!(
        reason = close_reason,
        duration_ms = started.elapsed().as_millis() as u64,
        frames_in_window = window.1,
        "relay session closed"
    );
    let frame = axum::extract::ws::CloseFrame {
        code: if close_reason == "rate_limited" {
            1008
        } else {
            1000
        },
        reason: close_reason.into(),
    };
    let _ = client_tx.send(Message::Close(Some(frame.clone()))).await;
    let _ = agent_tx.send(Message::Close(Some(frame))).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::SigningKey;

    fn signed(nonce: &[u8], key: &SigningKey, token: Option<&str>) -> Register {
        let mut message = LABEL_REGISTER.as_bytes().to_vec();
        message.extend_from_slice(nonce);
        let signature: Signature = key.sign(&message);
        let public = key.verifying_key().to_encoded_point(false);
        Register {
            t: "register".into(),
            v: 1,
            agent_pub: b64u(public.as_bytes()),
            sig: b64u(&signature.to_bytes()),
            token: token.map(str::to_string),
        }
    }

    #[test]
    fn registration_requires_a_valid_signature_over_the_nonce() {
        let key = SigningKey::random(&mut rand::rngs::OsRng);
        let nonce = [9u8; 32];
        let register = signed(&nonce, &key, None);
        let id = verify_registration(&register, &nonce, None).unwrap();
        assert!(is_agent_id(&id));
        assert_eq!(
            verify_registration(&register, &[8u8; 32], None),
            Err("bad_signature")
        );
        let other = SigningKey::random(&mut rand::rngs::OsRng);
        let mut forged = signed(&nonce, &other, None);
        forged.agent_pub = register.agent_pub.clone();
        assert_eq!(
            verify_registration(&forged, &nonce, None),
            Err("bad_signature")
        );
    }

    #[test]
    fn registration_enforces_the_access_token() {
        let key = SigningKey::random(&mut rand::rngs::OsRng);
        let nonce = [1u8; 32];
        assert_eq!(
            verify_registration(&signed(&nonce, &key, None), &nonce, Some("s3cret")),
            Err("forbidden")
        );
        assert_eq!(
            verify_registration(&signed(&nonce, &key, Some("nope")), &nonce, Some("s3cret")),
            Err("forbidden")
        );
        assert!(verify_registration(
            &signed(&nonce, &key, Some("s3cret")),
            &nonce,
            Some("s3cret")
        )
        .is_ok());
    }
}

#[derive(Deserialize)]
struct PairLookup {
    code: String,
}

async fn pair_handler(
    State(relay): State<Arc<Relay>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::Json(PairLookup { code }): axum::Json<PairLookup>,
) -> Response<Body> {
    let ip = client_ip(&relay, peer, &headers);
    if !relay.pairing_limiter.allow(ip) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let mut invitations = relay.invitations.lock();
    invitations.retain(|_, (_, _, expires, _)| *expires > crate::util::now_unix());
    let result = invitations
        .iter_mut()
        .find(|(_, (pin, _, _, attempts))| pin == &code && *attempts < 5);
    let Some((agent, (_, fingerprint, _, attempts))) = result else {
        return StatusCode::NOT_FOUND.into_response();
    };
    *attempts += 1;
    (
        [("cache-control", "no-store")],
        axum::Json(json!({"agentId":agent,"fingerprint":fingerprint})),
    )
        .into_response()
}
