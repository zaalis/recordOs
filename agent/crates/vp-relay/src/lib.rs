//! Vision-Pro Internet relay.
//!
//! Lets an iPad reach its paired PC when both are on different networks, without ever holding
//! keys: signaling stays end-to-end encrypted between the devices and media stays DTLS/SRTP.
//! See docs/RELAY.md for the protocol, the threat model and deployment.

pub mod http;
pub mod hub;
pub mod stun;
pub mod turn;
pub mod util;
pub mod wire;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::{mpsc, watch};
use turn::{ClientKey, Sink};

#[derive(Clone)]
pub struct RelayConfig {
    /// HTTP(S) listener for the WebSocket endpoints.
    pub http_bind: SocketAddr,
    /// TURN over UDP and TCP (same port when possible). Also carries the agent media tunnel.
    pub turn_bind: SocketAddr,
    /// TURN over TLS; requires `tls`.
    pub turn_tls_bind: Option<SocketAddr>,
    /// Certificate for HTTPS/WSS and TURN-TLS. Without it the HTTP listener is plain
    /// (intended to sit behind a TLS reverse proxy).
    pub tls: Option<Arc<rustls::ServerConfig>>,
    /// Hostname or IP address that iPads and agents use to reach this relay.
    pub public_host: String,
    /// When set, agents must present this token to register.
    pub access_token: Option<String>,
    /// Trust the last `X-Forwarded-For` hop (only behind a reverse proxy you control).
    pub trust_proxy: bool,
}

pub struct Relay {
    pub config: RelayConfig,
    pub hub: hub::Hub,
    pub turn: turn::Turn,
    pub turn_urls: Vec<String>,
    pub turn_udp_port: u16,
    pub register_limiter: util::RateLimiter,
    pub connect_limiter: util::RateLimiter,
    /// Flips to `true` on shutdown; long-lived connections watch it and close.
    pub stopping: watch::Receiver<bool>,
}

pub struct RunningRelay {
    pub relay: Arc<Relay>,
    pub http_addr: SocketAddr,
    pub turn_udp_addr: SocketAddr,
    pub turn_tcp_addr: SocketAddr,
    pub turn_tls_addr: Option<SocketAddr>,
    shutdown: watch::Sender<bool>,
}

impl RunningRelay {
    pub fn stop(&self) {
        let _ = self.shutdown.send(true);
    }
}

impl Drop for RunningRelay {
    fn drop(&mut self) {
        self.stop();
    }
}

fn host_for_url(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

pub async fn start(config: RelayConfig) -> anyhow::Result<RunningRelay> {
    let udp = Arc::new(UdpSocket::bind(config.turn_bind).await?);
    let turn_udp_addr = udp.local_addr()?;
    // Prefer the same port for TCP; fall back to the configured one (or any) if it is taken.
    let turn_tcp =
        match TcpListener::bind(SocketAddr::new(config.turn_bind.ip(), turn_udp_addr.port())).await
        {
            Ok(listener) => listener,
            Err(_) => TcpListener::bind(config.turn_bind).await?,
        };
    let turn_tcp_addr = turn_tcp.local_addr()?;
    let http = TcpListener::bind(config.http_bind).await?;
    let http_addr = http.local_addr()?;
    let turn_tls = match (&config.tls, config.turn_tls_bind) {
        (Some(tls), Some(bind)) => {
            let mut turn_config = (**tls).clone();
            turn_config.alpn_protocols.clear();
            Some((
                TcpListener::bind(bind).await?,
                tokio_rustls::TlsAcceptor::from(Arc::new(turn_config)),
            ))
        }
        _ => None,
    };
    let turn_tls_addr = match &turn_tls {
        Some((listener, _)) => Some(listener.local_addr()?),
        None => None,
    };

    let host = host_for_url(&config.public_host);
    let mut turn_urls = vec![
        format!("turn:{host}:{}?transport=udp", turn_udp_addr.port()),
        format!("turn:{host}:{}?transport=tcp", turn_tcp_addr.port()),
    ];
    if let Some(addr) = turn_tls_addr {
        turn_urls.push(format!("turns:{host}:{}?transport=tcp", addr.port()));
    }

    let http_tls = config.tls.clone().map(tokio_rustls::TlsAcceptor::from);
    let (shutdown, rx) = watch::channel(false);
    let relay = Arc::new(Relay {
        hub: hub::Hub::new(udp.clone()),
        turn: turn::Turn::new(),
        turn_urls,
        turn_udp_port: turn_udp_addr.port(),
        register_limiter: util::RateLimiter::new(20, 0.5),
        connect_limiter: util::RateLimiter::new(30, 1.0),
        stopping: rx.clone(),
        config,
    });

    tokio::spawn(udp_loop(relay.clone(), udp, rx.clone()));
    tokio::spawn(stream_accept_loop(
        relay.clone(),
        turn_tcp,
        None,
        rx.clone(),
    ));
    if let Some((listener, acceptor)) = turn_tls {
        tokio::spawn(stream_accept_loop(
            relay.clone(),
            listener,
            Some(acceptor),
            rx.clone(),
        ));
    }
    tokio::spawn(http::serve(relay.clone(), http, http_tls, rx.clone()));
    tokio::spawn(sweep_loop(relay.clone(), rx));

    Ok(RunningRelay {
        relay,
        http_addr,
        turn_udp_addr,
        turn_tcp_addr,
        turn_tls_addr,
        shutdown,
    })
}

async fn sweep_loop(relay: Arc<Relay>, mut shutdown: watch::Receiver<bool>) {
    let mut tick = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tick.tick() => {
                relay.turn.sweep();
                relay.hub.sweep();
            }
        }
    }
}

async fn udp_loop(relay: Arc<Relay>, socket: Arc<UdpSocket>, mut shutdown: watch::Receiver<bool>) {
    let mut buf = vec![0u8; 65_536];
    loop {
        let (n, from) = tokio::select! {
            _ = shutdown.changed() => break,
            received = socket.recv_from(&mut buf) => match received {
                Ok(pair) => pair,
                // Windows reports ICMP port-unreachable from earlier sends as a receive error.
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => continue,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    continue;
                }
            }
        };
        if n == 0 {
            continue;
        }
        let data = &buf[..n];
        if wire::is_tunnel(data[0]) {
            relay.hub.udp_frame(&relay, from, data);
        } else {
            let sink = Sink::Udp(socket.clone(), from);
            relay
                .turn
                .handle_client(&relay, ClientKey::Udp(from), from, &sink, data);
        }
    }
}

async fn stream_accept_loop(
    relay: Arc<Relay>,
    listener: TcpListener,
    tls: Option<tokio_rustls::TlsAcceptor>,
    mut shutdown: watch::Receiver<bool>,
) {
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
        let relay = relay.clone();
        let conn_id = relay.hub.next_id();
        match tls.clone() {
            Some(acceptor) => {
                tokio::spawn(async move {
                    if let Ok(Ok(stream)) =
                        tokio::time::timeout(Duration::from_secs(10), acceptor.accept(stream)).await
                    {
                        turn_stream(relay, stream, peer, conn_id).await;
                    }
                });
            }
            None => {
                tokio::spawn(turn_stream(relay, stream, peer, conn_id));
            }
        }
    }
}

enum Frame {
    Need,
    Ready { message: usize, total: usize },
    Invalid,
}

const MAX_STREAM_FRAME: usize = 8_192;

fn frame_length(buf: &[u8]) -> Frame {
    if buf.len() < 4 {
        return Frame::Need;
    }
    let length = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    match buf[0] & 0xC0 {
        0x00 => {
            let total = stun::HEADER_LEN + length;
            if total > MAX_STREAM_FRAME {
                Frame::Invalid
            } else {
                Frame::Ready {
                    message: total,
                    total,
                }
            }
        }
        0x40 => {
            let message = 4 + length;
            if message > MAX_STREAM_FRAME {
                Frame::Invalid
            } else {
                Frame::Ready {
                    message,
                    total: message.div_ceil(4) * 4,
                }
            }
        }
        _ => Frame::Invalid,
    }
}

/// TURN over a byte stream (TCP or TLS): STUN messages and padded ChannelData, back to back.
async fn turn_stream<S>(relay: Arc<Relay>, stream: S, peer: SocketAddr, conn_id: u64)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(1024);
    let writer_task = tokio::spawn(async move {
        while let Some(buffer) = rx.recv().await {
            if writer.write_all(&buffer).await.is_err() {
                break;
            }
        }
    });
    let key = ClientKey::Stream(conn_id);
    let sink = Sink::Stream(tx);
    let mut pending: Vec<u8> = Vec::with_capacity(16_384);
    let mut chunk = vec![0u8; 16_384];
    let mut stopping = relay.stopping.clone();
    'read: loop {
        let read = tokio::select! {
            _ = stopping.changed() => break,
            read = tokio::time::timeout(Duration::from_secs(120), reader.read(&mut chunk)) => read,
        };
        let n = match read {
            Ok(Ok(n)) if n > 0 => n,
            _ => break,
        };
        pending.extend_from_slice(&chunk[..n]);
        loop {
            match frame_length(&pending) {
                Frame::Need => break,
                Frame::Invalid => break 'read,
                Frame::Ready { message, total } => {
                    if pending.len() < total {
                        break;
                    }
                    relay
                        .turn
                        .handle_client(&relay, key, peer, &sink, &pending[..message]);
                    pending.drain(..total);
                }
            }
        }
    }
    relay.turn.drop_client(key);
    writer_task.abort();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_framing() {
        assert!(matches!(frame_length(&[0, 1]), Frame::Need));
        assert!(matches!(
            frame_length(&[0x00, 0x03, 0x00, 0x08]),
            Frame::Ready {
                message: 28,
                total: 28
            }
        ));
        assert!(matches!(
            frame_length(&[0x40, 0x00, 0x00, 0x05]),
            Frame::Ready {
                message: 9,
                total: 12
            }
        ));
        assert!(matches!(frame_length(&[0x80, 0, 0, 0]), Frame::Invalid));
        assert!(matches!(
            frame_length(&[0x40, 0x00, 0xff, 0xff]),
            Frame::Invalid
        ));
    }

    #[test]
    fn ipv6_hosts_are_bracketed() {
        assert_eq!(host_for_url("2001:db8::1"), "[2001:db8::1]");
        assert_eq!(host_for_url("relay.example.com"), "relay.example.com");
    }
}
