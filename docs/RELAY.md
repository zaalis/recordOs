# Remote Access relay

Remote Access lets a paired iPad control its Mac from any network. Both devices make outbound
connections to a Vision-Pro relay (`agent/crates/vp-relay`) that you operate. The relay routes
bytes; it never holds a key that could read the screen, the input or the signaling.

```text
iPad (any network)                     Relay (public host)                    Mac (home network)
  wss /v1/connect/<agentId>  ───────►  splice  ◄───────  wss /v1/accept/<ticket>
  TURN udp/tcp/tls :3478/5349 ───────► TURN ◄─ tunnel ─► UDP :3478 (WebSocket fallback)
                                        ▲
                                        └──── wss /v1/agent (challenge-signed registration)
```

## How a relayed session works

1. **Registration.** In Remote Access mode the Mac opens `wss://relay/v1/agent`, receives a random
   nonce and signs `"Vision-Pro/1 relay-register" || nonce` with its agent identity key. The relay
   derives the agent ID from the public key, so no Mac can register under another Mac's ID. A
   private relay can additionally require an access token.
2. **Learning the relay.** Every session on the local network includes `relay.url` inside the
   end-to-end encrypted `session.ready` message. The iPad stores it next to the pinned Mac key.
3. **Signaling.** Away from home the iPad opens `wss://relay/v1/connect/<agentId>`. The relay asks the
   Mac to dial `wss://relay/v1/accept/<ticket>` (192-bit, single use, 10 s) and splices both sockets.
   The unchanged Vision-Pro handshake then runs end to end: the iPad verifies the pinned agent key,
   so a malicious relay can only drop the connection.
4. **Media.** For relayed sessions the Mac asks the relay for a TURN credential bound to a fresh
   virtual peer address and sends it to the iPad inside the encrypted channel (`session.ready.ice`).
   The iPad uses `iceTransportPolicy: "relay"`; the Mac announces the virtual address as its only ICE
   candidate. Packets travel iPad ⇄ TURN allocation ⇄ tunnel ⇄ Mac. WebRTC DTLS-SRTP and SCTP
   protect video and input exactly as on the local network.

## Security properties

| Threat | Control |
| --- | --- |
| Relay reads or alters signaling | Handshake pinned to the agent key, AES-GCM frames, transcript signatures |
| Relay reads or alters video/input | DTLS fingerprints exchanged over the encrypted channel; DTLS-SRTP/SCTP |
| Relay impersonates the Mac | iPad rejects any agent key that differs from the pinned fingerprint |
| Someone registers as your Mac | Registration requires a signature by the agent private key |
| Strangers pair over the Internet | Nearby (code-less) pairing is refused on relayed sessions; a code or QR proof and approval on the Mac are required |
| Relay used as an open TURN proxy | Credentials are only issued to registered agents; permissions, channels and indications are restricted to that agent's leased virtual address |
| Brute force from the Internet | Relay rate limits per source; the Mac applies its lockout policy to the client IP reported by the relay |
| Unwanted exposure | Local Only mode never starts the relay client; the Mac never listens on a public port |

The relay operator can observe metadata: which agent IDs are online, client IP addresses, timing
and traffic volume.

## Deploying a relay

Requirements: a public host (VPS) with a DNS name, and these inbound ports:

| Port | Protocol | Purpose |
| --- | --- | --- |
| 443 | TCP | `wss://` signaling and agent registration |
| 3478 | UDP + TCP | TURN for iPads, media tunnel for Macs |
| 5349 | TCP | TURN over TLS (networks that only allow TLS) |

A certificate trusted by iPadOS is mandatory (for example Let's Encrypt):

```bash
vp-relay --host relay.example.com \
  --cert /etc/letsencrypt/live/relay.example.com/fullchain.pem \
  --key  /etc/letsencrypt/live/relay.example.com/privkey.pem \
  --token "$(openssl rand -base64 32)"
```

Every option can also be set through the environment (`VP_RELAY_HOST`, `VP_RELAY_CERT`,
`VP_RELAY_KEY`, `VP_RELAY_TOKEN`, `VP_RELAY_HTTP`, `VP_RELAY_TURN`, `VP_RELAY_TURN_TLS`,
`VP_RELAY_TRUST_PROXY=1`). Without a certificate the HTTP listener is plain (`0.0.0.0:8080`) and must
sit behind a TLS reverse proxy that forwards WebSocket upgrades; TURN ports are still exposed
directly. Use `--trust-proxy` only behind a proxy you control.

Build on Linux with Docker from the `agent/` directory:

```bash
docker build -f crates/vp-relay/Dockerfile -t vision-pro-relay .
docker run -d --restart unless-stopped -p 443:443 -p 3478:3478/udp -p 3478:3478 -p 5349:5349 \
  -v /etc/letsencrypt:/etc/letsencrypt:ro vision-pro-relay \
  --host relay.example.com --cert /etc/letsencrypt/live/relay.example.com/fullchain.pem \
  --key /etc/letsencrypt/live/relay.example.com/privkey.pem --token <secret>
```

Or natively: `cargo build --release -p vp-relay`.

## Configuring the Mac and the iPad

1. On the Mac: **Network → Remote Access**, enter `relay.example.com` and the access token, then save.
   The status shows *Reachable from the Internet* once registered (and whether media uses UDP).
2. Connect the iPad once on the local network. The next session records the relay.
3. Away from home, open Vision-Pro on the iPad: the Mac appears with **Via Internet**.

The cached iPad app keeps the Mac's Content-Security-Policy, which already allows the configured relay.
After changing the relay address, open the app once on the local network.

## Tests

- `cargo test -p vp-relay` — STUN codec (RFC 5769 vectors), registration, ticket splice, TURN over
  UDP/TCP, peer restriction, WebSocket media fallback.
- `cargo test -p vp-agent --test relay_link` — the real agent against the real relay: token refusal,
  registration, UDP tunnel, credentials, automatic reconnection, Local Only shutdown.
- `packages/protocol/test/e2e-relay.ts` — handshake through a running relay and agent.
