//! Media tunnel frames between an agent and the relay, carried as UDP datagrams or, when UDP is
//! blocked, as WebSocket binary messages on the agent control connection.
//!
//!   HELLO     A1 | token(32)                 agent → relay, binds the agent's UDP endpoint
//!   ACK       A2                             relay → agent
//!   DATA      A3 | peer(6) | relay(6) | data  both directions
//!   KEEPALIVE A4 | token(32)                 agent → relay
//!
//! `peer` is the agent's virtual address for one session, `relay` the iPad's TURN allocation.
//! The first byte never collides with STUN (0x00–0x3F) or ChannelData (0x40–0x7F).
//! Payloads are already protected end to end (DTLS/SRTP, STUN integrity); the relay only routes.

use std::net::{Ipv4Addr, SocketAddrV4};

pub const HELLO: u8 = 0xA1;
pub const ACK: u8 = 0xA2;
pub const DATA: u8 = 0xA3;
pub const KEEPALIVE: u8 = 0xA4;
pub const TOKEN_LEN: usize = 32;
pub const DATA_HEADER: usize = 13;

pub fn is_tunnel(first: u8) -> bool {
    first & 0xF0 == 0xA0
}

fn put_addr(out: &mut Vec<u8>, addr: SocketAddrV4) {
    out.extend_from_slice(&addr.ip().octets());
    out.extend_from_slice(&addr.port().to_be_bytes());
}

fn get_addr(bytes: &[u8]) -> SocketAddrV4 {
    SocketAddrV4::new(
        Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]),
        u16::from_be_bytes([bytes[4], bytes[5]]),
    )
}

pub fn encode_data(peer: SocketAddrV4, relay: SocketAddrV4, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(DATA_HEADER + payload.len());
    out.push(DATA);
    put_addr(&mut out, peer);
    put_addr(&mut out, relay);
    out.extend_from_slice(payload);
    out
}

pub fn decode_data(frame: &[u8]) -> Option<(SocketAddrV4, SocketAddrV4, &[u8])> {
    if frame.len() <= DATA_HEADER || frame[0] != DATA {
        return None;
    }
    Some((
        get_addr(&frame[1..7]),
        get_addr(&frame[7..13]),
        &frame[DATA_HEADER..],
    ))
}

pub fn encode_token_frame(kind: u8, token: &[u8; TOKEN_LEN]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + TOKEN_LEN);
    out.push(kind);
    out.extend_from_slice(token);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_roundtrip() {
        let peer: SocketAddrV4 = "100.64.0.7:9".parse().unwrap();
        let relay: SocketAddrV4 = "100.100.3.4:50000".parse().unwrap();
        let frame = encode_data(peer, relay, b"payload");
        assert!(is_tunnel(frame[0]));
        assert_eq!(decode_data(&frame), Some((peer, relay, &b"payload"[..])));
        assert_eq!(decode_data(&frame[..DATA_HEADER]), None);
        assert!(!is_tunnel(0x00) && !is_tunnel(0x40) && !is_tunnel(0x7f));
    }
}
