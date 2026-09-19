//! End-to-end relay tests over real sockets: agent registration, signaling splice, and TURN
//! allocations over UDP and TCP relayed through the agent media tunnel (UDP and WebSocket).

use futures_util::{SinkExt, StreamExt};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{json, Value};
use std::net::{SocketAddr, SocketAddrV4};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use vp_relay::stun::{attr, method, Builder, Class, Message as Stun};
use vp_relay::util::{b64u, from_b64u, random_bytes};
use vp_relay::{wire, RelayConfig, RunningRelay};

const TOKEN: &str = "0123456789abcdef-test";
type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

async fn relay() -> RunningRelay {
    vp_relay::start(RelayConfig {
        http_bind: "127.0.0.1:0".parse().unwrap(),
        turn_bind: "127.0.0.1:0".parse().unwrap(),
        turn_tls_bind: None,
        tls: None,
        public_host: "127.0.0.1".into(),
        access_token: Some(TOKEN.into()),
        trust_proxy: false,
    })
    .await
    .unwrap()
}

async fn next_json(ws: &mut Ws) -> Value {
    loop {
        match timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("ws timeout")
        {
            Some(Ok(Message::Text(text))) => return serde_json::from_str(&text).unwrap(),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            other => panic!("unexpected websocket event: {other:?}"),
        }
    }
}

struct Agent {
    ws: Ws,
    id: String,
    udp_token: [u8; 32],
}

async fn register(
    relay: &RunningRelay,
    key: &SigningKey,
    token: Option<&str>,
) -> Result<Agent, String> {
    let url = format!("ws://{}/v1/agent", relay.http_addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let challenge = next_json(&mut ws).await;
    assert_eq!(challenge["t"], "challenge");
    let nonce = from_b64u(challenge["nonce"].as_str().unwrap(), 32).unwrap();
    let mut message = b"Vision-Pro/1 relay-register".to_vec();
    message.extend_from_slice(&nonce);
    let signature: Signature = key.sign(&message);
    let public = key.verifying_key().to_encoded_point(false);
    let mut register = json!({ "t": "register", "v": 1, "agentPub": b64u(public.as_bytes()), "sig": b64u(&signature.to_bytes()) });
    if let Some(token) = token {
        register["token"] = json!(token);
    }
    ws.send(Message::Text(register.to_string().into()))
        .await
        .unwrap();
    let reply = next_json(&mut ws).await;
    if reply["t"] != "registered" {
        return Err(reply["code"].as_str().unwrap_or("?").to_string());
    }
    let udp_token: [u8; 32] = from_b64u(reply["udpToken"].as_str().unwrap(), 32)
        .unwrap()
        .try_into()
        .unwrap();
    Ok(Agent {
        ws,
        id: reply["agentId"].as_str().unwrap().to_string(),
        udp_token,
    })
}

struct Issued {
    username: String,
    password: String,
    peer: SocketAddrV4,
}

async fn issue(agent: &mut Agent, id: u32) -> Issued {
    agent
        .ws
        .send(Message::Text(
            json!({ "t": "turn.issue", "id": id, "ttl": 600 })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let reply = next_json(&mut agent.ws).await;
    assert_eq!(reply["t"], "turn.credential", "{reply}");
    assert_eq!(reply["id"], id);
    Issued {
        username: reply["username"].as_str().unwrap().into(),
        password: reply["credential"].as_str().unwrap().into(),
        peer: reply["peer"].as_str().unwrap().parse().unwrap(),
    }
}

/// Minimal TURN client used to exercise the server the way libwebrtc does.
struct TurnClient {
    realm: String,
    nonce: String,
    key: [u8; 16],
    username: String,
}

impl TurnClient {
    fn request(&self, method: u16, fill: impl FnOnce(&mut Builder)) -> (Vec<u8>, [u8; 12]) {
        let txid: [u8; 12] = random_bytes();
        let mut b = Builder::new(method, Class::Request, txid);
        fill(&mut b);
        b.attr(attr::USERNAME, self.username.as_bytes());
        b.attr(attr::REALM, self.realm.as_bytes());
        b.attr(attr::NONCE, self.nonce.as_bytes());
        b.integrity(&self.key);
        b.fingerprint();
        (b.finish(), txid)
    }
}

fn allocate_unauthenticated() -> Vec<u8> {
    let mut b = Builder::new(method::ALLOCATE, Class::Request, random_bytes());
    b.attr(attr::REQUESTED_TRANSPORT, &[17, 0, 0, 0]);
    b.fingerprint();
    b.finish()
}

fn error_code(message: &Stun) -> Option<u16> {
    message
        .get(attr::ERROR_CODE)
        .map(|v| v[2] as u16 * 100 + v[3] as u16)
}

async fn udp_exchange(socket: &UdpSocket, to: SocketAddr, request: &[u8]) -> Vec<u8> {
    socket.send_to(request, to).await.unwrap();
    let mut buf = vec![0u8; 2048];
    let (n, _) = timeout(Duration::from_secs(3), socket.recv_from(&mut buf))
        .await
        .expect("udp timeout")
        .unwrap();
    buf.truncate(n);
    buf
}

async fn recv_udp(socket: &UdpSocket) -> Vec<u8> {
    let mut buf = vec![0u8; 2048];
    let (n, _) = timeout(Duration::from_secs(3), socket.recv_from(&mut buf))
        .await
        .expect("udp timeout")
        .unwrap();
    buf.truncate(n);
    buf
}

async fn no_udp(socket: &UdpSocket) {
    let mut buf = vec![0u8; 2048];
    assert!(
        timeout(Duration::from_millis(300), socket.recv_from(&mut buf))
            .await
            .is_err(),
        "unexpected datagram"
    );
}

/// Allocates on the relay with the issued credential and returns the client and relayed address.
async fn udp_allocation(
    relay: &RunningRelay,
    socket: &UdpSocket,
    issued: &Issued,
) -> (TurnClient, SocketAddrV4) {
    let challenge = udp_exchange(socket, relay.turn_udp_addr, &allocate_unauthenticated()).await;
    let challenge = Stun::parse(&challenge).unwrap();
    assert_eq!(error_code(&challenge), Some(401));
    let realm = challenge.text(attr::REALM).unwrap().to_string();
    let nonce = challenge.text(attr::NONCE).unwrap().to_string();
    let client = TurnClient {
        key: vp_relay::stun::long_term_key(&issued.username, &realm, &issued.password),
        realm,
        nonce,
        username: issued.username.clone(),
    };
    let (request, txid) = client.request(method::ALLOCATE, |b| {
        b.attr(attr::REQUESTED_TRANSPORT, &[17, 0, 0, 0]);
    });
    let raw = udp_exchange(socket, relay.turn_udp_addr, &request).await;
    let response = Stun::parse(&raw).unwrap();
    assert_eq!(
        response.class,
        Class::Success,
        "allocate failed: {:?}",
        error_code(&response)
    );
    assert_eq!(response.txid, txid);
    assert!(
        response.verify_integrity(&client.key),
        "responses must carry MESSAGE-INTEGRITY"
    );
    assert!(response.fingerprint_ok());
    assert_eq!(
        response.xor_address(attr::XOR_MAPPED_ADDRESS),
        Some(socket.local_addr().unwrap())
    );
    let Some(SocketAddr::V4(relayed)) = response.xor_address(attr::XOR_RELAYED_ADDRESS) else {
        panic!("no relayed address")
    };
    (client, relayed)
}

#[tokio::test]
async fn agent_registration_is_authenticated() {
    let relay = relay().await;
    let key = SigningKey::random(&mut rand::rngs::OsRng);
    assert_eq!(
        register(&relay, &key, None).await.err().as_deref(),
        Some("forbidden")
    );
    assert_eq!(
        register(&relay, &key, Some("wrong-token-000000"))
            .await
            .err()
            .as_deref(),
        Some("forbidden")
    );
    let agent = register(&relay, &key, Some(TOKEN)).await.unwrap();
    assert_eq!(agent.id.len(), 22);
    assert!(relay.relay.hub.is_online(&agent.id));

    // A second registration with the same key replaces the first connection.
    let mut first = agent;
    let _second = register(&relay, &key, Some(TOKEN)).await.unwrap();
    let closed = timeout(Duration::from_secs(3), async {
        loop {
            match first.ws.next().await {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => continue,
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "replaced agent connection must be closed");
}

#[tokio::test]
async fn signaling_is_spliced_through_a_one_time_ticket() {
    let relay = relay().await;
    let key = SigningKey::random(&mut rand::rngs::OsRng);
    let mut agent = register(&relay, &key, Some(TOKEN)).await.unwrap();

    // Unknown agent: explicit offline error.
    let (mut stranger, _) = tokio_tungstenite::connect_async(format!(
        "ws://{}/v1/connect/{}",
        relay.http_addr,
        "A".repeat(22)
    ))
    .await
    .unwrap();
    assert_eq!(next_json(&mut stranger).await["code"], "offline");

    let connect = format!("ws://{}/v1/connect/{}", relay.http_addr, agent.id);
    let client_task =
        tokio::spawn(async move { tokio_tungstenite::connect_async(connect).await.unwrap().0 });
    let incoming = next_json(&mut agent.ws).await;
    assert_eq!(incoming["t"], "incoming");
    assert_eq!(incoming["ip"], "127.0.0.1");
    let ticket = incoming["ticket"].as_str().unwrap().to_string();
    let accept = format!("ws://{}/v1/accept/{ticket}", relay.http_addr);
    let (mut agent_data, _) = tokio_tungstenite::connect_async(accept.clone())
        .await
        .unwrap();
    let mut client = client_task.await.unwrap();

    agent_data
        .send(Message::Text(r#"{"t":"hello"}"#.into()))
        .await
        .unwrap();
    assert_eq!(next_json(&mut client).await["t"], "hello");
    client
        .send(Message::Text(r#"{"t":"init"}"#.into()))
        .await
        .unwrap();
    assert_eq!(next_json(&mut agent_data).await["t"], "init");

    // Tickets are single use.
    assert!(tokio_tungstenite::connect_async(accept).await.is_err());

    // Closing one side closes the other.
    client.close(None).await.unwrap();
    let ended = timeout(Duration::from_secs(3), async {
        loop {
            match agent_data.next().await {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => continue,
            }
        }
    })
    .await;
    assert!(ended.is_ok());
}

#[tokio::test]
async fn turn_udp_relays_only_to_the_leased_agent() {
    let relay = relay().await;
    let key = SigningKey::random(&mut rand::rngs::OsRng);
    let mut agent = register(&relay, &key, Some(TOKEN)).await.unwrap();

    let agent_udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ack = udp_exchange(
        &agent_udp,
        relay.turn_udp_addr,
        &wire::encode_token_frame(wire::HELLO, &agent.udp_token),
    )
    .await;
    assert_eq!(ack, vec![wire::ACK]);
    let keepalive = udp_exchange(
        &agent_udp,
        relay.turn_udp_addr,
        &wire::encode_token_frame(wire::KEEPALIVE, &agent.udp_token),
    )
    .await;
    assert_eq!(keepalive, vec![wire::ACK]);
    let unknown = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    unknown
        .send_to(
            &wire::encode_token_frame(wire::HELLO, &[0u8; 32]),
            relay.turn_udp_addr,
        )
        .await
        .unwrap();
    no_udp(&unknown).await;

    let issued = issue(&mut agent, 1).await;
    assert!(
        issued.peer.port() >= 1024,
        "browsers drop remote candidates on low ports"
    );
    let client_udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (client, relayed) = udp_allocation(&relay, &client_udp, &issued).await;

    // Permissions towards anything but the leased peer are forbidden.
    let outsider: SocketAddr = "93.184.216.34:443".parse().unwrap();
    let (request, _) = client.request(method::CREATE_PERMISSION, |b| {
        b.xor_address(attr::XOR_PEER_ADDRESS, outsider);
    });
    let response = udp_exchange(&client_udp, relay.turn_udp_addr, &request).await;
    assert_eq!(error_code(&Stun::parse(&response).unwrap()), Some(403));

    // Without a permission nothing is forwarded.
    let mut send = Builder::new(method::SEND, Class::Indication, random_bytes());
    send.xor_address(attr::XOR_PEER_ADDRESS, SocketAddr::V4(issued.peer));
    send.attr(attr::DATA, b"early");
    client_udp
        .send_to(&send.finish(), relay.turn_udp_addr)
        .await
        .unwrap();
    no_udp(&agent_udp).await;

    let (request, _) = client.request(method::CREATE_PERMISSION, |b| {
        b.xor_address(attr::XOR_PEER_ADDRESS, SocketAddr::V4(issued.peer));
    });
    let response = udp_exchange(&client_udp, relay.turn_udp_addr, &request).await;
    let response = Stun::parse(&response).unwrap();
    assert_eq!(response.class, Class::Success);
    assert!(response.verify_integrity(&client.key));

    // iPad → agent via Send indication.
    let mut send = Builder::new(method::SEND, Class::Indication, random_bytes());
    send.xor_address(attr::XOR_PEER_ADDRESS, SocketAddr::V4(issued.peer));
    send.attr(attr::DATA, b"ping");
    client_udp
        .send_to(&send.finish(), relay.turn_udp_addr)
        .await
        .unwrap();
    let frame = recv_udp(&agent_udp).await;
    assert_eq!(
        wire::decode_data(&frame),
        Some((issued.peer, relayed, &b"ping"[..]))
    );

    // Agent → iPad via Data indication.
    agent_udp
        .send_to(
            &wire::encode_data(issued.peer, relayed, b"pong"),
            relay.turn_udp_addr,
        )
        .await
        .unwrap();
    let raw = recv_udp(&client_udp).await;
    let data = Stun::parse(&raw).unwrap();
    assert_eq!((data.method, data.class), (method::DATA, Class::Indication));
    assert_eq!(
        data.xor_address(attr::XOR_PEER_ADDRESS),
        Some(SocketAddr::V4(issued.peer))
    );
    assert_eq!(data.get(attr::DATA), Some(&b"pong"[..]));

    // The agent cannot inject into allocations of other peers.
    let foreign: SocketAddrV4 = "100.64.200.1:9".parse().unwrap();
    agent_udp
        .send_to(
            &wire::encode_data(foreign, relayed, b"spoof"),
            relay.turn_udp_addr,
        )
        .await
        .unwrap();
    no_udp(&client_udp).await;

    // Channels: bind, then ChannelData in both directions.
    let (request, _) = client.request(method::CHANNEL_BIND, |b| {
        b.attr(attr::CHANNEL_NUMBER, &[0x40, 0x01, 0, 0]);
        b.xor_address(attr::XOR_PEER_ADDRESS, SocketAddr::V4(issued.peer));
    });
    let response = udp_exchange(&client_udp, relay.turn_udp_addr, &request).await;
    assert_eq!(Stun::parse(&response).unwrap().class, Class::Success);
    agent_udp
        .send_to(
            &wire::encode_data(issued.peer, relayed, b"video"),
            relay.turn_udp_addr,
        )
        .await
        .unwrap();
    assert_eq!(
        recv_udp(&client_udp).await,
        [&[0x40, 0x01, 0x00, 0x05][..], b"video"].concat()
    );
    client_udp
        .send_to(
            &[&[0x40, 0x01, 0x00, 0x03][..], b"ack"].concat(),
            relay.turn_udp_addr,
        )
        .await
        .unwrap();
    assert_eq!(
        wire::decode_data(&recv_udp(&agent_udp).await),
        Some((issued.peer, relayed, &b"ack"[..]))
    );

    // A tampered credential is refused.
    let other_client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let forged = Issued {
        username: issued.username.clone(),
        password: "not-the-password".into(),
        peer: issued.peer,
    };
    let challenge = Stun::parse(
        &udp_exchange(
            &other_client,
            relay.turn_udp_addr,
            &allocate_unauthenticated(),
        )
        .await,
    )
    .unwrap()
    .text(attr::NONCE)
    .unwrap()
    .to_string();
    let bad = TurnClient {
        key: vp_relay::stun::long_term_key(&forged.username, "vision-pro", &forged.password),
        realm: "vision-pro".into(),
        nonce: challenge,
        username: forged.username,
    };
    let (request, _) = bad.request(method::ALLOCATE, |b| {
        b.attr(attr::REQUESTED_TRANSPORT, &[17, 0, 0, 0]);
    });
    assert_eq!(
        error_code(
            &Stun::parse(&udp_exchange(&other_client, relay.turn_udp_addr, &request).await)
                .unwrap()
        ),
        Some(401)
    );

    // Releasing the lease tears the allocation down.
    agent
        .ws
        .send(Message::Text(
            json!({ "t": "turn.release", "peer": issued.peer.to_string() })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    client_udp
        .send_to(
            &[&[0x40, 0x01, 0x00, 0x03][..], b"bye"].concat(),
            relay.turn_udp_addr,
        )
        .await
        .unwrap();
    no_udp(&agent_udp).await;
    assert_eq!(relay.relay.turn.allocation_count(), 0);
}

#[tokio::test]
async fn turn_tcp_and_websocket_media_fallback() {
    let relay = relay().await;
    let key = SigningKey::random(&mut rand::rngs::OsRng);
    // No UDP hello: media must reach the agent over its WebSocket.
    let mut agent = register(&relay, &key, Some(TOKEN)).await.unwrap();
    let issued = issue(&mut agent, 7).await;

    let mut tcp = TcpStream::connect(relay.turn_tcp_addr).await.unwrap();
    async fn exchange(tcp: &mut TcpStream, request: &[u8]) -> Vec<u8> {
        tcp.write_all(request).await.unwrap();
        let mut header = [0u8; 20];
        timeout(Duration::from_secs(3), tcp.read_exact(&mut header))
            .await
            .unwrap()
            .unwrap();
        let length = u16::from_be_bytes([header[2], header[3]]) as usize;
        let mut body = vec![0u8; length];
        tcp.read_exact(&mut body).await.unwrap();
        [header.to_vec(), body].concat()
    }
    let challenge = exchange(&mut tcp, &allocate_unauthenticated()).await;
    let challenge = Stun::parse(&challenge).unwrap();
    let client = TurnClient {
        key: vp_relay::stun::long_term_key(&issued.username, "vision-pro", &issued.password),
        realm: challenge.text(attr::REALM).unwrap().into(),
        nonce: challenge.text(attr::NONCE).unwrap().into(),
        username: issued.username.clone(),
    };
    let (request, _) = client.request(method::ALLOCATE, |b| {
        b.attr(attr::REQUESTED_TRANSPORT, &[17, 0, 0, 0]);
    });
    let response = exchange(&mut tcp, &request).await;
    let response = Stun::parse(&response).unwrap();
    assert_eq!(response.class, Class::Success);
    let Some(SocketAddr::V4(relayed)) = response.xor_address(attr::XOR_RELAYED_ADDRESS) else {
        panic!()
    };
    let (request, _) = client.request(method::CHANNEL_BIND, |b| {
        b.attr(attr::CHANNEL_NUMBER, &[0x40, 0x10, 0, 0]);
        b.xor_address(attr::XOR_PEER_ADDRESS, SocketAddr::V4(issued.peer));
    });
    assert_eq!(
        Stun::parse(&exchange(&mut tcp, &request).await)
            .unwrap()
            .class,
        Class::Success
    );

    // iPad → agent: ChannelData over TCP (padded) arrives as a WebSocket binary frame.
    tcp.write_all(&[
        0x40, 0x10, 0x00, 0x05, b'h', b'e', b'l', b'l', b'o', 0, 0, 0,
    ])
    .await
    .unwrap();
    let frame = loop {
        match timeout(Duration::from_secs(3), agent.ws.next())
            .await
            .unwrap()
        {
            Some(Ok(Message::Binary(frame))) => break frame,
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            other => panic!("expected binary media frame, got {other:?}"),
        }
    };
    assert_eq!(
        wire::decode_data(&frame),
        Some((issued.peer, relayed, &b"hello"[..]))
    );

    // Agent → iPad over WebSocket, delivered as padded ChannelData on the TCP allocation.
    agent
        .ws
        .send(Message::Binary(
            wire::encode_data(issued.peer, relayed, b"world").into(),
        ))
        .await
        .unwrap();
    let mut buf = [0u8; 12];
    timeout(Duration::from_secs(3), tcp.read_exact(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        buf,
        [0x40, 0x10, 0x00, 0x05, b'w', b'o', b'r', b'l', b'd', 0, 0, 0]
    );

    // Disconnecting the agent removes its leases and allocations.
    agent.ws.close(None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!relay.relay.hub.is_online(&agent.id));
    assert_eq!(relay.relay.turn.allocation_count(), 0);
}
