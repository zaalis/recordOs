//! Registry of connected agents, one-time accept tickets and the agent media tunnel.

use crate::util::random_token;
use crate::wire;
use crate::Relay;
use axum::extract::ws::WebSocket;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot};

const TICKET_TTL: Duration = Duration::from_secs(10);
const MAX_PENDING_PER_AGENT: usize = 16;
const MAX_AGENTS: usize = 10_000;
/// Without a keepalive for this long, media falls back to the WebSocket control connection.
const UDP_FRESH: Duration = Duration::from_secs(45);

pub enum Outgoing {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

struct AgentEntry {
    conn_id: u64,
    tx: mpsc::Sender<Outgoing>,
    udp_token: [u8; wire::TOKEN_LEN],
    udp: Option<(SocketAddr, Instant)>,
}

struct Ticket {
    agent_id: String,
    created: Instant,
    tx: oneshot::Sender<WebSocket>,
}

#[derive(Default)]
struct Inner {
    agents: HashMap<String, AgentEntry>,
    udp_index: HashMap<SocketAddr, String>,
    tickets: HashMap<String, Ticket>,
}

pub struct Registration {
    pub conn_id: u64,
    pub rx: mpsc::Receiver<Outgoing>,
    pub tx: mpsc::Sender<Outgoing>,
    pub udp_token: [u8; wire::TOKEN_LEN],
}

pub struct Hub {
    inner: Mutex<Inner>,
    next_id: AtomicU64,
    udp: Arc<UdpSocket>,
}

impl Hub {
    pub fn new(udp: Arc<UdpSocket>) -> Hub {
        Hub {
            inner: Mutex::new(Inner::default()),
            next_id: AtomicU64::new(1),
            udp,
        }
    }

    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Registers an authenticated agent, replacing (and closing) any previous connection.
    pub fn register(&self, agent_id: &str) -> Option<Registration> {
        let (tx, rx) = mpsc::channel(4096);
        let conn_id = self.next_id();
        let udp_token = crate::util::random_bytes();
        let mut inner = self.inner.lock();
        if inner.agents.len() >= MAX_AGENTS && !inner.agents.contains_key(agent_id) {
            return None;
        }
        let previous = inner.agents.insert(
            agent_id.to_string(),
            AgentEntry {
                conn_id,
                tx: tx.clone(),
                udp_token,
                udp: None,
            },
        );
        if let Some(previous) = previous {
            if let Some((addr, _)) = previous.udp {
                inner.udp_index.remove(&addr);
            }
            let _ = previous.tx.try_send(Outgoing::Close);
        }
        Some(Registration {
            conn_id,
            rx,
            tx,
            udp_token,
        })
    }

    pub fn unregister(&self, agent_id: &str, conn_id: u64) {
        let mut inner = self.inner.lock();
        if inner
            .agents
            .get(agent_id)
            .is_some_and(|a| a.conn_id == conn_id)
        {
            if let Some(entry) = inner.agents.remove(agent_id) {
                if let Some((addr, _)) = entry.udp {
                    inner.udp_index.remove(&addr);
                }
            }
            inner.tickets.retain(|_, t| t.agent_id != agent_id);
        }
    }

    pub fn is_online(&self, agent_id: &str) -> bool {
        self.inner.lock().agents.contains_key(agent_id)
    }

    pub fn agent_count(&self) -> usize {
        self.inner.lock().agents.len()
    }

    /// Asks the agent to open a data connection for a new client.
    pub fn request_accept(
        &self,
        agent_id: &str,
        ip: IpAddr,
    ) -> Option<oneshot::Receiver<WebSocket>> {
        let mut inner = self.inner.lock();
        let pending = inner
            .tickets
            .values()
            .filter(|t| t.agent_id == agent_id && t.created.elapsed() < TICKET_TTL)
            .count();
        if pending >= MAX_PENDING_PER_AGENT {
            return None;
        }
        let agent_tx = inner.agents.get(agent_id)?.tx.clone();
        let ticket = random_token(24);
        let (tx, rx) = oneshot::channel();
        let message =
            serde_json::json!({ "t": "incoming", "ticket": ticket, "ip": ip.to_string() });
        agent_tx
            .try_send(Outgoing::Text(message.to_string()))
            .ok()?;
        inner.tickets.insert(
            ticket,
            Ticket {
                agent_id: agent_id.to_string(),
                created: Instant::now(),
                tx,
            },
        );
        Some(rx)
    }

    pub fn take_ticket(&self, ticket: &str) -> Option<oneshot::Sender<WebSocket>> {
        let entry = self.inner.lock().tickets.remove(ticket)?;
        (entry.created.elapsed() < TICKET_TTL).then_some(entry.tx)
    }

    pub fn sweep(&self) {
        let mut inner = self.inner.lock();
        inner
            .tickets
            .retain(|_, t| t.created.elapsed() < TICKET_TTL);
        let stale: Vec<SocketAddr> = inner
            .agents
            .values()
            .filter_map(|a| a.udp.filter(|(_, at)| at.elapsed() > UDP_FRESH * 4))
            .map(|(addr, _)| addr)
            .collect();
        for addr in stale {
            if let Some(agent_id) = inner.udp_index.remove(&addr) {
                if let Some(agent) = inner.agents.get_mut(&agent_id) {
                    agent.udp = None;
                }
            }
        }
    }

    /// Forwards a media frame to an agent over UDP when its endpoint is fresh, else over WebSocket.
    pub fn to_agent(&self, agent_id: &str, conn_id: u64, frame: Vec<u8>) {
        let inner = self.inner.lock();
        let Some(agent) = inner.agents.get(agent_id).filter(|a| a.conn_id == conn_id) else {
            return;
        };
        match agent.udp {
            Some((addr, at)) if at.elapsed() < UDP_FRESH => {
                let _ = self.udp.try_send_to(&frame, addr);
            }
            _ => {
                let _ = agent.tx.try_send(Outgoing::Binary(frame));
            }
        }
    }

    /// Handles a tunnel datagram received on the TURN UDP port.
    pub fn udp_frame(&self, relay: &Relay, from: SocketAddr, frame: &[u8]) {
        match frame[0] {
            wire::HELLO | wire::KEEPALIVE if frame.len() == 1 + wire::TOKEN_LEN => {
                if self.bind_udp(from, &frame[1..]) {
                    let _ = self.udp.try_send_to(&[wire::ACK], from);
                }
            }
            wire::DATA => {
                let owner = {
                    let inner = self.inner.lock();
                    inner.udp_index.get(&from).and_then(|agent_id| {
                        inner
                            .agents
                            .get(agent_id)
                            .map(|a| (agent_id.clone(), a.conn_id))
                    })
                };
                if let (Some((agent_id, conn_id)), Some((peer, relay_addr, payload))) =
                    (owner, wire::decode_data(frame))
                {
                    relay
                        .turn
                        .deliver_from_agent(&agent_id, conn_id, peer, relay_addr, payload);
                }
            }
            _ => {}
        }
    }

    fn bind_udp(&self, from: SocketAddr, token: &[u8]) -> bool {
        let mut inner = self.inner.lock();
        let Some(agent_id) = inner
            .agents
            .iter()
            .find(|(_, a)| bool::from(a.udp_token.as_slice().ct_eq(token)))
            .map(|(id, _)| id.clone())
        else {
            return false;
        };
        let previous = inner
            .agents
            .get(&agent_id)
            .and_then(|a| a.udp)
            .map(|(addr, _)| addr);
        if let Some(previous) = previous.filter(|p| *p != from) {
            inner.udp_index.remove(&previous);
        }
        inner.udp_index.insert(from, agent_id.clone());
        if let Some(agent) = inner.agents.get_mut(&agent_id) {
            agent.udp = Some((from, Instant::now()));
        }
        true
    }
}
