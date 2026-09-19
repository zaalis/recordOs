//! Local servers:
//! - HTTPS (TLS 1.3) on the LAN: web app + `/ws` signaling. Local Only mode drops non-LAN peers
//!   before the TLS handshake.
//! - HTTP bootstrap: only while "Connect Device" is open; serves the trust profile, nothing else.
//! - Developer loopback (opt-in): plain HTTP bound to 127.0.0.1, for testing in a desktop browser.

use crate::agent::Core;
use crate::config::NetworkMode;
use crate::security::{Severity, Verdict};
use crate::signaling::{self, Route, SignalTransport};
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, HeaderValue, Request, Response, StatusCode};
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Extension;
use axum::{Json, Router};
use hyper_util::rt::TokioIo;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tower::ServiceExt;

pub const BOOTSTRAP_PORT: u16 = 47801;
pub const DEV_LOOPBACK_PORT: u16 = 47802;
const MAX_WS_MESSAGE: usize = 96 * 1024;

#[derive(rust_embed::Embed)]
#[folder = "../../../apps/ipad/dist/"]
#[exclude = "*.map"]
struct WebAssets;

pub struct ServerHandle {
    shutdown: watch::Sender<bool>,
}

/// Local address a connection arrived on (a Wi-Fi address, or a USB cable address).
#[derive(Clone, Copy)]
pub struct LocalAddr(pub SocketAddr);

impl Drop for ServerHandle {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

pub async fn start(core: Arc<Core>) -> anyhow::Result<ServerHandle> {
    let (shutdown, rx) = watch::channel(false);
    let settings = core.settings.read().clone();
    let port = settings.network.https_port;

    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)))
        .await
        .map_err(|e| anyhow::anyhow!("port {port} is unavailable: {e}"))?;
    tokio::spawn(accept_loop(core.clone(), listener, true, rx.clone()));

    if settings.advanced.developer_loopback {
        match TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, DEV_LOOPBACK_PORT))).await {
            Ok(dev) => {
                core.audit.record(
                    Severity::Warning,
                    "service.dev-loopback",
                    "Developer loopback endpoint enabled on 127.0.0.1.",
                );
                tokio::spawn(accept_loop(core.clone(), dev, false, rx.clone()));
            }
            Err(error) => tracing::warn!(%error, "developer loopback unavailable"),
        }
        // A simulated cable (tests) is another loopback address with its own listener.
        let simulated = core.usb_simulation.read().map(|s| s.ip);
        if let Some(ip) = simulated.filter(|ip| ip.is_loopback() && *ip != Ipv4Addr::LOCALHOST) {
            match TcpListener::bind(SocketAddr::from((ip, DEV_LOOPBACK_PORT))).await {
                Ok(dev) => {
                    tokio::spawn(accept_loop(core.clone(), dev, false, rx.clone()));
                }
                Err(error) => tracing::warn!(%error, "simulated USB listener unavailable"),
            }
        }
    }
    Ok(ServerHandle { shutdown })
}

/// Starts the bootstrap listener; it stops when the returned handle is dropped.
pub async fn start_bootstrap(core: Arc<Core>) -> anyhow::Result<ServerHandle> {
    let (shutdown, rx) = watch::channel(false);
    let listener =
        TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, BOOTSTRAP_PORT))).await?;
    let router = bootstrap_router(core.clone());
    tokio::spawn(async move {
        let mut rx = rx;
        loop {
            tokio::select! {
                _ = rx.changed() => break,
                accepted = listener.accept() => {
                    let Ok((stream, peer)) = accepted else { continue };
                    if !crate::net::is_local_address(peer.ip()) || core.guard.check_connection(peer.ip()) != Verdict::Allow {
                        continue;
                    }
                    let router = router.clone();
                    let local = stream.local_addr().unwrap_or(SocketAddr::from((Ipv4Addr::UNSPECIFIED, BOOTSTRAP_PORT)));
                    tokio::spawn(serve_http(TokioIo::new(stream), peer, local, router, false));
                }
            }
        }
    });
    Ok(ServerHandle { shutdown })
}

static NONLOCAL_LOG_BUDGET: AtomicU64 = AtomicU64::new(0);

async fn accept_loop(
    core: Arc<Core>,
    listener: TcpListener,
    tls: bool,
    mut shutdown: watch::Receiver<bool>,
) {
    let router = app_router(core.clone(), tls);
    loop {
        let (stream, peer) = tokio::select! {
            _ = shutdown.changed() => break,
            accepted = listener.accept() => match accepted {
                Ok(pair) => pair,
                Err(error) => {
                    tracing::warn!(%error, "accept failed");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            }
        };
        let ip = peer.ip();
        let local_only = core.settings.read().network.mode == NetworkMode::LocalOnly;
        if local_only && !crate::net::is_local_address(ip) {
            // Log at most once a minute to avoid log flooding.
            let minute = crate::util::now_ms() / 60_000;
            if NONLOCAL_LOG_BUDGET.swap(minute, Ordering::Relaxed) != minute {
                core.audit
                    .record(
                        Severity::Warning,
                        "network.blocked",
                        "Blocked a connection from outside the local network.",
                    )
                    .ip(ip);
            }
            continue;
        }
        if let Verdict::LockedOut(_) | Verdict::RateLimited(_) = core.guard.check_connection(ip) {
            continue;
        }
        let _ = stream.set_nodelay(true);
        let Ok(local) = stream.local_addr() else {
            continue;
        };
        let router = router.clone();
        if tls {
            let Some(config) = core.tls.read().clone() else {
                continue;
            };
            tokio::spawn(async move {
                let acceptor = tokio_rustls::TlsAcceptor::from(config);
                if let Ok(Ok(tls_stream)) =
                    tokio::time::timeout(Duration::from_secs(10), acceptor.accept(stream)).await
                {
                    serve_http(TokioIo::new(tls_stream), peer, local, router, true).await
                }
            });
        } else {
            tokio::spawn(serve_http(TokioIo::new(stream), peer, local, router, false));
        }
    }
}

async fn serve_http<I>(
    io: TokioIo<I>,
    peer: SocketAddr,
    local: SocketAddr,
    router: Router,
    _tls: bool,
) where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = hyper::service::service_fn(move |mut request: Request<hyper::body::Incoming>| {
        request.extensions_mut().insert(ConnectInfo(peer));
        request.extensions_mut().insert(LocalAddr(local));
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

fn app_router(core: Arc<Core>, tls: bool) -> Router {
    let relay_core = core.clone();
    Router::new()
        .route("/ws", get(ws_handler))
        .route("/api/info", get(info_handler))
        .route("/api/nearby", get(nearby_handler))
        .fallback(get(static_handler))
        .layer(middleware::from_fn(move |req, next| {
            // The cached iPad app keeps this policy while away from home, so it must already allow
            // the configured relay.
            let (relay, port) = {
                let settings = relay_core.settings.read();
                (
                    settings.network.relay_url.clone(),
                    settings.network.https_port,
                )
            };
            let policy = if tls {
                Policy::App(relay, port)
            } else {
                Policy::DevApp(relay, DEV_LOOPBACK_PORT)
            };
            security_headers(req, next, policy)
        }))
        .with_state(core)
}

#[derive(Clone)]
enum Policy {
    /// Web app; the port is the agent's signaling port on any address, because the installed app
    /// keeps its `.local` origin while it reaches the PC through a cable address.
    App(Option<String>, u16),
    DevApp(Option<String>, u16),
    Bootstrap(String),
}

async fn security_headers(request: Request<Body>, next: Next, policy: Policy) -> Response<Body> {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .filter(|h| {
            h.len() < 128
                && h.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '[' | ']'))
        })
        .map(str::to_string);
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    let csp = match &policy {
        Policy::App(relay, port) | Policy::DevApp(relay, port) => {
            let ws_scheme = if matches!(policy, Policy::App(..)) { "wss" } else { "ws" };
            let mut connect = host.map(|h| format!(" {ws_scheme}://{h}")).unwrap_or_default();
            connect.push_str(&format!(" {ws_scheme}://*:{port}"));
            if let Some(relay) = relay {
                connect.push(' ');
                connect.push_str(relay);
            }
            format!(
                "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data: blob:; media-src 'self' blob:; \
                 connect-src 'self'{connect}; worker-src 'self'; manifest-src 'self'; font-src 'self' data:; object-src 'none'; \
                 base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
            )
        }
        Policy::Bootstrap(target) => format!(
            "default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; connect-src {target}; img-src 'self'; \
             base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
        ),
    };
    if let Ok(value) = HeaderValue::from_str(&csp) {
        headers.insert(header::CONTENT_SECURITY_POLICY, value);
    }
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        "cross-origin-opener-policy",
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static(
            "camera=(self), microphone=(), geolocation=(), payment=(), usb=(), fullscreen=(self)",
        ),
    );
    response
}

fn origin_allowed(core: &Core, origin: &str) -> bool {
    let Some(rest) = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
    else {
        return false;
    };
    let host = rest.rsplit_once(':').map(|(h, _)| h).unwrap_or(rest);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if origin.starts_with("http://") {
        // Plain HTTP only exists on the developer loopback.
        return core.settings.read().advanced.developer_loopback
            && (host == "127.0.0.1" || host == "localhost");
    }
    host == core.hostname
        || host == "localhost"
        || host == "127.0.0.1"
        || host
            .parse::<IpAddr>()
            .is_ok_and(|ip| core.cert_ips.read().iter().any(|c| IpAddr::V4(*c) == ip))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(core): State<Arc<Core>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(LocalAddr(local)): Extension<LocalAddr>,
    headers: HeaderMap,
) -> Response<Body> {
    if let Some(origin) = headers.get(header::ORIGIN) {
        let allowed = origin.to_str().is_ok_and(|o| origin_allowed(&core, o));
        if !allowed {
            core.audit
                .record(
                    Severity::Warning,
                    "network.bad-origin",
                    "Rejected a WebSocket from a foreign web origin.",
                )
                .ip(peer.ip());
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    ws.max_message_size(MAX_WS_MESSAGE)
        .max_frame_size(MAX_WS_MESSAGE)
        .on_upgrade(move |socket| {
            signaling::run(
                core,
                AxumTransport(socket),
                peer.ip(),
                Route::Direct,
                Some(local.ip()),
            )
        })
}

struct AxumTransport(WebSocket);

impl SignalTransport for AxumTransport {
    async fn send_text(&mut self, text: String) -> anyhow::Result<()> {
        self.0.send(Message::Text(text.into())).await?;
        Ok(())
    }

    async fn recv_text(&mut self) -> Option<anyhow::Result<String>> {
        loop {
            match self.0.recv().await? {
                Ok(Message::Text(text)) => return Some(Ok(text.to_string())),
                Ok(Message::Binary(_)) => {
                    return Some(Err(anyhow::anyhow!("binary signaling not allowed")))
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => continue,
                Ok(Message::Close(_)) => return None,
                Err(error) => return Some(Err(error.into())),
            }
        }
    }

    async fn close(&mut self) {
        let _ = self.0.send(Message::Close(None)).await;
    }
}

async fn info_handler(State(core): State<Arc<Core>>) -> impl IntoResponse {
    let identity = core.identity.read().clone();
    let settings = core.settings.read();
    let body = serde_json::json!({
        "product": crate::PRODUCT_NAME,
        "version": crate::VERSION,
        "agentId": identity.agent_id,
        "name": settings.computer_name,
        "fingerprint": identity.fingerprint,
        "hostname": core.hostname,
        "port": settings.network.https_port,
        "pairing": { "nearby": settings.network.allow_nearby_pairing },
        "usb": { "urls": core.usb.read().urls() },
    });
    ([(header::CACHE_CONTROL, "no-store")], Json(body))
}

async fn nearby_handler(State(core): State<Arc<Core>>) -> impl IntoResponse {
    let list = core.nearby.read().clone();
    ([(header::CACHE_CONTROL, "no-store")], Json(list))
}

async fn static_handler(request: Request<Body>) -> Response<Body> {
    let path = request.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    if path.contains("..") || path.contains('\\') {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let (asset, served_path) = match WebAssets::get(path) {
        Some(asset) => (Some(asset), path.to_string()),
        None if !path.rsplit('/').next().unwrap_or("").contains('.') => {
            (WebAssets::get("index.html"), "index.html".to_string())
        }
        None => (None, path.to_string()),
    };
    let Some(asset) = asset else {
        if served_path == "index.html" {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                "Vision-Pro web app is not built. Run `npm run build:web`.",
            )
                .into_response();
        }
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = mime_guess::from_path(&served_path).first_or_octet_stream();
    let cache = if served_path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, cache)
        .body(Body::from(asset.data.into_owned()))
        .unwrap()
}

fn bootstrap_router(core: Arc<Core>) -> Router {
    let target = format!(
        "https://{}:{}",
        core.hostname,
        core.settings.read().network.https_port
    );
    Router::new()
        .route("/", get(bootstrap_page))
        .route("/setup.js", get(bootstrap_script))
        .route("/vision-pro.mobileconfig", get(bootstrap_profile))
        .layer(middleware::from_fn(move |req, next| {
            security_headers(req, next, Policy::Bootstrap(target.clone()))
        }))
        .with_state(core)
}

async fn bootstrap_page(State(core): State<Arc<Core>>) -> impl IntoResponse {
    let authority_fp = core
        .authority
        .read()
        .as_ref()
        .map(|a| a.fingerprint_sha256())
        .unwrap_or_default();
    let settings = core.settings.read().clone();
    let target = format!("https://{}:{}/", core.hostname, settings.network.https_port);
    let html = include_str!("bootstrap.html")
        .replace("{{NAME}}", &html_escape(&settings.computer_name))
        .replace("{{TARGET}}", &html_escape(&target))
        .replace("{{FINGERPRINT}}", &html_escape(&authority_fp));
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        html,
    )
}

async fn bootstrap_script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("bootstrap.js"),
    )
}

async fn bootstrap_profile(
    State(core): State<Arc<Core>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response<Body> {
    let Some(authority) = core.authority.read().clone() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let name = core.settings.read().computer_name.clone();
    core.audit
        .record(
            Severity::Info,
            "trust.profile-downloaded",
            "The local trust profile was downloaded.",
        )
        .ip(peer.ip());
    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-apple-aspen-config")
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"Vision-Pro.mobileconfig\"",
        )
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(authority.mobileconfig(&name)))
        .unwrap()
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
