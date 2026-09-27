//! Signaling session: authenticated key exchange, authorization (pairing / trusted device),
//! then an end-to-end encrypted control channel that negotiates WebRTC.
//!
//! Handshake (see docs/PROTOCOL.md):
//!   S→C hello{agentPub, commit}   C→S init{clientEph, nC, devicePub…}
//!   S→C reveal{serverEph, nS, sig_agent(T)}   C→S prove{sig_device(T)}
//! Every failure is counted per source address and can trigger a lockout.

use crate::agent::Core;
use crate::crypto::{self, EncryptedFrame, Role, SecureChannel};
use crate::events::{AgentEvent, PairingRequestInfo};
use crate::media::{MediaContext, MediaHandle, MediaRequest};
use crate::protocol::{ClientInit, ClientProve, DeviceRotate, PairRequest, RtcOffer};
use crate::security::Severity;
use crate::session::{EndReason, SessionCommand, SessionInfo};
use crate::store::{DeviceStatus, Permission};
use crate::util::{b64u, from_b64u, now_ms, random_bytes, random_token, sanitize_name};
use p256::ecdh::EphemeralSecret;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use serde_json::{json, Value};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{timeout, Instant};

const STEP_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_FRAME: usize = 96 * 1024;

/// Minimal transport abstraction so the same session logic runs over the local WebSocket or a relay.
pub trait SignalTransport: Send {
    fn send_text(
        &mut self,
        text: String,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
    /// `None` when the peer closed the transport.
    fn recv_text(
        &mut self,
    ) -> impl std::future::Future<Output = Option<anyhow::Result<String>>> + Send;
    fn close(&mut self) -> impl std::future::Future<Output = ()> + Send;
}

#[derive(Debug, thiserror::Error)]
enum Abort {
    #[error("{0}")]
    Client(&'static str),
    #[error("transport closed")]
    Closed,
}

struct Ctx<'a, T: SignalTransport> {
    core: &'a Arc<Core>,
    transport: &'a mut T,
    ip: IpAddr,
}

impl<T: SignalTransport> Ctx<'_, T> {
    async fn send_plain(&mut self, value: Value) -> Result<(), Abort> {
        self.transport
            .send_text(value.to_string())
            .await
            .map_err(|_| Abort::Closed)
    }

    async fn fail(&mut self, code: &'static str, retry_after: Option<Duration>) -> Abort {
        let mut message = json!({ "t": "error", "code": code });
        if let Some(d) = retry_after {
            message["retryAfterMs"] = json!(d.as_millis() as u64);
        }
        let _ = self.send_plain(message).await;
        Abort::Client(code)
    }

    /// Counts a failure for this source; emits a lockout audit entry when the threshold is reached.
    fn penalize(&self, reason: &str) {
        if let Some(duration) = self.core.guard.record_failure(self.ip) {
            self.core
                .audit
                .record(
                    Severity::Critical,
                    "security.lockout",
                    format!(
                        "Too many failed attempts ({reason}). Source blocked for {} min.",
                        duration.as_secs() / 60
                    ),
                )
                .ip(self.ip);
        }
    }

    async fn recv_plain(&mut self) -> Result<Value, Abort> {
        let text = match timeout(STEP_TIMEOUT, self.transport.recv_text()).await {
            Ok(Some(Ok(text))) => text,
            Ok(_) => return Err(Abort::Closed),
            Err(_) => return Err(self.fail("expired", None).await),
        };
        if text.len() > MAX_FRAME {
            self.penalize("oversized message");
            return Err(self.fail("protocol", None).await);
        }
        match serde_json::from_str::<Value>(&text) {
            Ok(value) => Ok(value),
            Err(_) => {
                self.penalize("malformed message");
                Err(self.fail("protocol", None).await)
            }
        }
    }
}

async fn send_secure<T: SignalTransport>(
    transport: &mut T,
    channel: &mut SecureChannel,
    value: Value,
) -> Result<(), Abort> {
    for frame in channel.seal(&value) {
        let text = serde_json::to_string(&frame).expect("frame json");
        transport.send_text(text).await.map_err(|_| Abort::Closed)?;
    }
    Ok(())
}

enum Incoming {
    Message(Value),
    Closed,
    Violation(&'static str),
}

fn open_secure(channel: &mut SecureChannel, text: &str) -> Incoming {
    if text.len() > MAX_FRAME {
        return Incoming::Violation("oversized frame");
    }
    let Ok(frame) = serde_json::from_str::<EncryptedFrame>(text) else {
        return Incoming::Violation("malformed frame");
    };
    match channel.open(&frame) {
        Ok(Some(value)) => Incoming::Message(value),
        Ok(None) => Incoming::Message(json!({ "t": "__rekey" })),
        Err(_) => Incoming::Violation("frame authentication"),
    }
}

async fn recv_secure<T: SignalTransport>(
    transport: &mut T,
    channel: &mut SecureChannel,
    wait: Duration,
) -> Incoming {
    loop {
        let text = match timeout(wait, transport.recv_text()).await {
            Ok(Some(Ok(text))) => text,
            Ok(_) => return Incoming::Closed,
            Err(_) => return Incoming::Violation("timeout"),
        };
        match open_secure(channel, &text) {
            Incoming::Message(v) if v["t"] == "__rekey" => continue,
            other => return other,
        }
    }
}

pub async fn run<T: SignalTransport>(core: Arc<Core>, mut transport: T, ip: IpAddr) {
    let Ok(_permit) = core.handshake_slots.clone().try_acquire_owned() else {
        let _ = transport
            .send_text(
                json!({"t": "error", "code": "rate_limited", "retryAfterMs": 2000}).to_string(),
            )
            .await;
        transport.close().await;
        return;
    };
    let result = session(&core, &mut transport, ip, _permit).await;
    if let Err(Abort::Client(code)) = &result {
        tracing::debug!(%ip, code, "signaling ended");
    }
    transport.close().await;
}

async fn session<T: SignalTransport>(
    core: &Arc<Core>,
    transport: &mut T,
    ip: IpAddr,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<(), Abort> {
    let mut ctx = Ctx {
        core,
        transport,
        ip,
    };
    if let Some(remaining) = core.guard.locked_for(ip) {
        return Err(ctx.fail("locked_out", Some(remaining)).await);
    }

    let identity = core.identity.read().clone();
    let settings = core.settings.read().clone();

    // --- hello (server commits to its ephemeral first) ---
    let server_eph = EphemeralSecret::random(&mut rand::rngs::OsRng);
    let server_eph_raw = server_eph
        .public_key()
        .to_encoded_point(false)
        .as_bytes()
        .to_vec();
    let server_nonce = random_bytes::<32>();
    let commit = crypto::commit(&server_eph_raw, &server_nonce);
    let window_open = core.pairing.window().is_some();
    ctx.send_plain(json!({
        "t": "hello",
        "v": crate::protocol::PROTOCOL_VERSION,
        "agentId": identity.agent_id,
        "agentName": settings.computer_name,
        "agentPub": b64u(identity.public_raw()),
        "commit": b64u(&commit),
        "pairing": { "nearby": settings.network.allow_nearby_pairing, "code": window_open, "qr": window_open },
    }))
    .await?;

    // --- init ---
    let value = ctx.recv_plain().await?;
    let init: ClientInit = match serde_json::from_value::<ClientInit>(value) {
        Ok(init) if init.validate().is_ok() => init,
        Ok(init) if init.v != crate::protocol::PROTOCOL_VERSION => {
            return Err(ctx.fail("version", None).await)
        }
        _ => {
            ctx.penalize("invalid init");
            return Err(ctx.fail("protocol", None).await);
        }
    };
    let (Some(client_eph_raw), Some(client_nonce), Some(device_pub)) = (
        from_b64u(&init.client_eph, Some(65)),
        from_b64u(&init.n_c, Some(32)),
        from_b64u(&init.device_pub, Some(65)),
    ) else {
        ctx.penalize("invalid init encoding");
        return Err(ctx.fail("protocol", None).await);
    };
    let (Some(device_key), Ok(client_eph)) = (
        crypto::parse_public_key(&device_pub),
        p256::PublicKey::from_sec1_bytes(&client_eph_raw),
    ) else {
        ctx.penalize("invalid keys");
        return Err(ctx.fail("protocol", None).await);
    };
    if crypto::id_from_public_key(&device_pub) != init.device_id {
        ctx.penalize("device id mismatch");
        return Err(ctx.fail("protocol", None).await);
    }

    // --- reveal ---
    let transcript = crypto::transcript(&crypto::TranscriptInput {
        agent_id: &identity.agent_id,
        agent_pub: identity.public_raw(),
        commit: &commit,
        mode: &init.mode,
        client_eph: &client_eph_raw,
        client_nonce: &client_nonce,
        device_id: &init.device_id,
        device_pub: &device_pub,
        device_name: &init.device_name,
        device_kind: &init.device_kind,
        server_eph: &server_eph_raw,
        server_nonce: &server_nonce,
    });
    let agent_sig = identity.sign(crypto::LABEL_AGENT_SIG, &transcript);
    ctx.send_plain(json!({ "t": "reveal", "serverEph": b64u(&server_eph_raw), "nS": b64u(&server_nonce), "sig": b64u(&agent_sig) }))
        .await?;

    // --- prove ---
    let value = ctx.recv_plain().await?;
    let prove = match serde_json::from_value::<ClientProve>(value) {
        Ok(p) if p.t == "prove" => p,
        _ => {
            ctx.penalize("invalid prove");
            return Err(ctx.fail("protocol", None).await);
        }
    };
    let sig_ok = from_b64u(&prove.sig, Some(64)).is_some_and(|sig| {
        crypto::verify(&device_key, crypto::LABEL_DEVICE_SIG, &transcript, &sig)
    });
    if !sig_ok {
        ctx.penalize("bad device signature");
        core.audit
            .record(
                Severity::Warning,
                "auth.bad-signature",
                "Rejected a connection with an invalid device signature.",
            )
            .ip(ip);
        return Err(ctx.fail("bad_signature", None).await);
    }

    let shared = server_eph.diffie_hellman(&client_eph);
    let keys = crypto::derive_session_keys(shared.raw_secret_bytes().as_slice(), transcript);
    drop(shared);
    let mut channel = SecureChannel::new(&keys, Role::Server);

    let device_name = sanitize_name(&init.device_name, 48);
    let record = core.devices.read().get(&init.device_id).cloned();
    let device_pub_b64 = b64u(&device_pub);

    // --- authorization ---
    let mut needs_approval: Option<&'static str> = None;
    match (init.mode.as_str(), &record) {
        (_, Some(r)) if r.status == DeviceStatus::Revoked => {
            ctx.penalize("revoked device");
            core.audit
                .record(
                    Severity::Warning,
                    "auth.revoked",
                    "A revoked device tried to connect.",
                )
                .ip(ip)
                .device(&r.id, &r.name);
            return Err(ctx.fail("revoked", None).await);
        }
        (_, Some(r)) if r.public_key != device_pub_b64 => {
            ctx.penalize("key mismatch");
            return Err(ctx.fail("unknown_device", None).await);
        }
        ("auth", None) => {
            ctx.penalize("unknown device");
            core.audit
                .record(
                    Severity::Notice,
                    "auth.unknown-device",
                    "An unpaired device tried to connect.",
                )
                .ip(ip);
            return Err(ctx.fail("unknown_device", None).await);
        }
        (_, Some(r)) if r.status == DeviceStatus::Untrusted => needs_approval = Some("reconfirm"),
        (_, Some(_)) if settings.security.confirm_every_connection => {
            needs_approval = Some("reconfirm")
        }
        (_, Some(_)) => {}
        ("pair", None) => {
            let request = match recv_secure(ctx.transport, &mut channel, STEP_TIMEOUT).await {
                Incoming::Message(v) => serde_json::from_value::<PairRequest>(v)
                    .ok()
                    .filter(|r| r.t == "pair.request"),
                Incoming::Closed => return Err(Abort::Closed),
                Incoming::Violation(_) => None,
            };
            let Some(request) = request else {
                ctx.penalize("invalid pair request");
                return Err(ctx.fail("protocol", None).await);
            };
            let method = match request.proof {
                Some(proof) => match core
                    .pairing
                    .verify_proof(&proof.kind, &proof.mac, &transcript)
                {
                    Ok(method) => method,
                    Err(_) => {
                        ctx.penalize("invalid pairing proof");
                        core.audit
                            .record(
                                Severity::Warning,
                                "pair.bad-proof",
                                "Rejected a pairing attempt with an invalid code.",
                            )
                            .ip(ip);
                        return Err(ctx.fail("denied", None).await);
                    }
                },
                None if settings.network.allow_nearby_pairing => "nearby",
                None => return Err(ctx.fail("pairing_disabled", None).await),
            };
            needs_approval = Some(method);
        }
        _ => {
            ctx.penalize("invalid mode");
            return Err(ctx.fail("protocol", None).await);
        }
    }

    if let Some(method) = needs_approval {
        let timeout_secs = settings.security.pairing_timeout_seconds as u64;
        let info = PairingRequestInfo {
            id: random_token(9),
            device_id: init.device_id.clone(),
            device_name: record
                .as_ref()
                .map(|r| r.name.clone())
                .unwrap_or_else(|| device_name.clone()),
            device_kind: init.device_kind.clone(),
            ip: ip.to_string(),
            sas: keys.sas.clone(),
            method: method.to_string(),
            created_at: now_ms(),
            expires_at: now_ms() + timeout_secs * 1000,
        };
        let Some(decision) = core.pairing.begin(info.clone()) else {
            return Err(ctx
                .fail("pairing_busy", Some(Duration::from_secs(10)))
                .await);
        };
        core.audit
            .record(
                Severity::Notice,
                "pair.requested",
                format!("{} requested access ({method}).", info.device_name),
            )
            .ip(ip)
            .device(&info.device_id, &info.device_name);
        let _ = core.events.send(AgentEvent::PairingRequested(info.clone()));
        send_secure(
            ctx.transport,
            &mut channel,
            json!({ "t": "pair.pending", "expiresInMs": timeout_secs * 1000 }),
        )
        .await?;

        // Wait for the Mac user, the timeout, or the iPad giving up — whichever comes first.
        let outcome = tokio::select! {
            result = decision => if result.unwrap_or(false) { "approved" } else { "denied" },
            _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => "expired",
            _ = wait_for_close(ctx.transport, &mut channel) => "cancelled",
        };
        core.pairing.clear(&info.id);
        let _ = core.events.send(AgentEvent::PairingResolved {
            id: info.id.clone(),
            outcome: outcome.to_string(),
        });

        if outcome != "approved" {
            let severity = if outcome == "denied" {
                Severity::Warning
            } else {
                Severity::Info
            };
            core.audit
                .record(
                    severity,
                    "pair.not-approved",
                    format!("Access for {} was {outcome}.", info.device_name),
                )
                .ip(ip)
                .device(&info.device_id, &info.device_name);
            if outcome == "denied" {
                if let Some(duration) = core.guard.record_pair_denied(ip) {
                    core.audit
                        .record(
                            Severity::Critical,
                            "security.lockout",
                            format!(
                                "Repeated denied requests. Source blocked for {} min.",
                                duration.as_secs() / 60
                            ),
                        )
                        .ip(ip);
                }
            }
            let _ = send_secure(
                ctx.transport,
                &mut channel,
                json!({ "t": "pair.result", "approved": false }),
            )
            .await;
            return Err(Abort::Client("denied"));
        }

        {
            let mut devices = core.devices.write();
            if method == "reconfirm" {
                devices.set_trusted(&init.device_id);
            } else {
                devices.insert_paired(
                    &init.device_id,
                    &device_name,
                    &init.device_kind,
                    &device_pub,
                    method,
                    &ip.to_string(),
                );
            }
            if let Err(error) = devices.save(&identity) {
                tracing::error!(%error, "failed to persist device store");
            }
        }
        if method == "qr" {
            core.pairing.consume_token();
        }
        core.audit
            .record(
                Severity::Notice,
                "pair.approved",
                format!("{} is now a trusted device ({method}).", info.device_name),
            )
            .ip(ip)
            .device(&info.device_id, &info.device_name);
        let _ = core.events.send(AgentEvent::DevicesChanged);
        send_secure(
            ctx.transport,
            &mut channel,
            json!({ "t": "pair.result", "approved": true }),
        )
        .await?;
    }

    core.guard.record_success(ip);
    drop(permit);
    run_session(core, ctx.transport, channel, ip, &init, &keys.session_id).await
}

/// Resolves when the transport closes (used while waiting for an approval).
async fn wait_for_close<T: SignalTransport>(transport: &mut T, channel: &mut SecureChannel) {
    loop {
        match transport.recv_text().await {
            None | Some(Err(_)) => return,
            Some(Ok(text)) => {
                if let Incoming::Message(v) = open_secure(channel, &text) {
                    if v["t"] == "session.close" {
                        return;
                    }
                }
            }
        }
    }
}

async fn run_session<T: SignalTransport>(
    core: &Arc<Core>,
    transport: &mut T,
    mut channel: SecureChannel,
    ip: IpAddr,
    init: &ClientInit,
    session_id: &str,
) -> Result<(), Abort> {
    let identity = core.identity.read().clone();
    let settings = core.settings.read().clone();
    let record = {
        let mut devices = core.devices.write();
        devices.touch(&init.device_id, &ip.to_string());
        let _ = devices.save(&identity);
        devices.get(&init.device_id).cloned()
    };
    let Some(record) = record else {
        return Err(Abort::Client("device vanished"));
    };

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<SessionCommand>(8);
    let info = core.sessions.register(
        SessionInfo {
            id: session_id.to_string(),
            device_id: record.id.clone(),
            device_name: record.name.clone(),
            device_kind: record.kind.clone(),
            ip: ip.to_string(),
            started_at: now_ms(),
            permission: record.permission,
            streaming: false,
            video: None,
            stats: None,
            rtt_ms: None,
            active_app: None,
        },
        cmd_tx,
    );
    let _ = core.events.send(AgentEvent::SessionsChanged);
    if settings.security.notify_on_connect {
        core.audit
            .record(
                Severity::Notice,
                "session.started",
                format!("{} connected.", record.name),
            )
            .ip(ip)
            .device(&record.id, &record.name);
    }

    let control_allowed = Arc::new(AtomicBool::new(record.permission == Permission::Control));
    let max_duration = Duration::from_secs(settings.security.session_max_minutes as u64 * 60);
    let expires_at = now_ms() + max_duration.as_millis() as u64;

    let result = async {
        send_secure(
            transport,
            &mut channel,
            json!({
                "t": "session.ready",
                "sessionId": session_id,
                "device": { "id": record.id, "name": record.name, "permission": record.permission },
                "agent": {
                    "id": identity.agent_id,
                    "name": settings.computer_name,
                    "version": crate::VERSION,
                    "hostname": core.hostname,
                },
                "caps": core.media.capabilities(),
                "expiresAt": expires_at,
            }),
        )
        .await?;

        let mut media: Option<Box<dyn MediaHandle>> = None;
        let deadline = Instant::now() + max_duration;
        let reason = loop {
            tokio::select! {
                incoming = transport.recv_text() => {
                    let text = match incoming {
                        Some(Ok(text)) => text,
                        _ => break EndReason::Stopped,
                    };
                    let message = match open_secure(&mut channel, &text) {
                        Incoming::Message(v) => v,
                        Incoming::Closed => break EndReason::Stopped,
                        Incoming::Violation(why) => {
                            core.audit.record(Severity::Warning, "session.violation", format!("Session ended: {why}.")).ip(ip).device(&record.id, &record.name);
                            core.guard.record_failure(ip);
                            break EndReason::Error;
                        }
                    };
                    match message["t"].as_str() {
                        Some("__rekey") => {}
                        Some("rtc.offer") => {
                            let offer = match serde_json::from_value::<RtcOffer>(message) {
                                Ok(o) if o.t == "rtc.offer" && o.validate().is_ok() => o,
                                _ => {
                                    core.guard.record_failure(ip);
                                    break EndReason::Error;
                                }
                            };
                            if let Some(mut old) = media.take() {
                                old.stop();
                            }
                            let context = MediaContext {
                                control_allowed: control_allowed.clone(),
                                info: info.clone(),
                                settings: core.settings.read().clone(),
                                on_change: {
                                    let events = core.events.clone();
                                    Arc::new(move || { let _ = events.send(AgentEvent::SessionsChanged); })
                                },
                            };
                            let request = MediaRequest { offer, session_id: session_id.to_string(), device_name: record.name.clone() };
                            let engine = core.media.clone();
                            let started = tokio::task::spawn_blocking(move || engine.start(request, context)).await;
                            match started {
                                Ok(Ok(started)) => {
                                    {
                                        let mut i = info.write();
                                        i.streaming = true;
                                        i.video = Some(started.applied.clone());
                                    }
                                    let _ = core.events.send(AgentEvent::SessionsChanged);
                                    send_secure(transport, &mut channel, json!({ "t": "rtc.answer", "sdp": started.answer_sdp, "video": started.applied })).await?;
                                    media = Some(started.handle);
                                }
                                Ok(Err(error)) => {
                                    tracing::error!(%error, "media start failed");
                                    core.audit.record(Severity::Warning, "media.error", format!("Streaming could not start: {error}")).device(&record.id, &record.name);
                                    send_secure(transport, &mut channel, json!({ "t": "rtc.error", "message": "The Mac could not start streaming." })).await?;
                                }
                                Err(_) => break EndReason::Error,
                            }
                        }
                        Some("session.close") => break EndReason::User,
                        Some("device.rotate") => {
                            let Ok(rotate) = serde_json::from_value::<DeviceRotate>(message) else { break EndReason::Error };
                            let new_pub = from_b64u(&rotate.new_pub, Some(65));
                            let valid = new_pub.as_ref().and_then(|p| crypto::parse_public_key(p)).zip(from_b64u(&rotate.sig, Some(64))).is_some_and(|(key, sig)| {
                                crypto::verify(&key, crypto::LABEL_ROTATE, &crypto::rotation_message(session_id, new_pub.as_ref().unwrap()), &sig)
                            });
                            if !valid {
                                core.guard.record_failure(ip);
                                break EndReason::Error;
                            }
                            let new_id = {
                                let mut devices = core.devices.write();
                                let id = devices.rotate_key(&record.id, new_pub.as_ref().unwrap());
                                let _ = devices.save(&identity);
                                id
                            };
                            match new_id {
                                Some(id) => {
                                    core.audit.record(Severity::Notice, "device.key-rotated", format!("{} rotated its device key.", record.name)).device(&id, &record.name);
                                    let _ = core.events.send(AgentEvent::DevicesChanged);
                                    send_secure(transport, &mut channel, json!({ "t": "device.rotated", "deviceId": id })).await?;
                                    // The session was authenticated with the old key; end it so the next one uses the new key.
                                    break EndReason::Replaced;
                                }
                                None => break EndReason::Error,
                            }
                        }
                        _ => {
                            core.guard.record_failure(ip);
                            break EndReason::Error;
                        }
                    }
                }
                command = cmd_rx.recv() => match command {
                    Some(SessionCommand::End(reason)) => break reason,
                    Some(SessionCommand::SetPermission(permission)) => {
                        control_allowed.store(permission == Permission::Control, Ordering::SeqCst);
                        info.write().permission = permission;
                        let _ = core.events.send(AgentEvent::SessionsChanged);
                        send_secure(transport, &mut channel, json!({ "t": "session.permission", "control": permission == Permission::Control })).await?;
                    }
                    None => break EndReason::Shutdown,
                },
                _ = tokio::time::sleep_until(deadline) => break EndReason::Expired,
            }
        };
        if let Some(mut handle) = media.take() {
            handle.stop();
        }
        let _ = send_secure(transport, &mut channel, json!({ "t": "session.end", "reason": reason.as_str() })).await;
        Ok::<EndReason, Abort>(reason)
    }
    .await;

    core.sessions.unregister(session_id);
    let _ = core.events.send(AgentEvent::SessionsChanged);
    let reason = result.as_ref().map(|r| r.as_str()).unwrap_or("closed");
    core.audit
        .record(
            Severity::Info,
            "session.ended",
            format!("{} disconnected ({reason}).", record.name),
        )
        .ip(ip)
        .device(&record.id, &record.name);
    result.map(|_| ())
}
