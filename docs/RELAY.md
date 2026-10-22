# Remote Access relay

Remote Access lets a paired iPad control its PC from any network. Both devices make outbound
connections to a Vision-Pro relay (`agent/crates/vp-relay`) that you operate. The relay routes
bytes; it never holds a key that could read the input or the session.

> Simpler alternative: put the PC and the iPad on the same [Tailscale](https://tailscale.com)
> network. "Connect Device" then shows the PC's Tailscale address, and the iPad reaches the PC from
> anywhere as if it were on the local network — no relay, no public host, no open port.

```text
iPad (any network)                     Relay (public host)                    PC (home network)
  wss /v1/connect/<agentId>  ───────►  splice  ◄───────  wss /v1/accept/<ticket>
                                        ▲
                                        └──── wss /v1/agent (challenge-signed registration)
```

## How a relayed session works

1. **Registration.** In Remote Access mode the PC opens `wss://relay/v1/agent`, receives a random
   nonce and signs `"Vision-Pro/1 relay-register" || nonce` with its agent identity key. The relay
   derives the agent ID from the public key, so no PC can register under another PC's ID. A
   private relay can additionally require an access token.
2. **Learning the relay.** Every session on the local network includes `relay.url` inside the
   end-to-end encrypted `session.ready` message. The iPad stores it next to the pinned PC key.
3. **Signaling.** Away from home the iPad opens `wss://relay/v1/connect/<agentId>`. The relay asks the
   PC to dial `wss://relay/v1/accept/<ticket>` (192-bit, single use, 10 s) and splices both sockets.
   The unchanged Vision-Pro handshake then runs end to end: the iPad verifies the pinned agent key,
   so a malicious relay can only drop the connection.
4. **Input.** Nothing is streamed from the PC: the iPad's input travels inside the same encrypted
   session, through the same splice. No media path, TURN or UDP is involved.

## Security properties

| Threat | Control |
| --- | --- |
| Relay reads or alters signaling | Handshake pinned to the agent key, AES-GCM frames, transcript signatures |
| Relay reads or alters input | Input is inside the end-to-end encrypted session (AES-GCM, strict sequence numbers) |
| Relay impersonates the PC | iPad rejects any agent key that differs from the pinned fingerprint |
| Someone registers as your PC | Registration requires a signature by the agent private key |
| Strangers pair over the Internet | Pairing always requires the code shown on the PC |
| Brute force from the Internet | Relay rate limits per source; the PC applies its lockout policy to the client IP reported by the relay |
| Unwanted exposure | Local Only mode never starts the relay client; the PC never listens on a public port |

The relay operator can observe metadata: which agent IDs are online, client IP addresses, timing
and traffic volume.

## Deploying a relay

Requirements: a public host (VPS) with a DNS name, and inbound TCP 443 (`wss://` sessions and agent
registration). The relay binary still opens its TURN ports (3478, 5349) from the screen-streaming
versions; current PCs and iPads never use them, so they do not need to be reachable.

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

## Configuring the PC and the iPad

1. On the PC: **Network → Remote Access**, enter `relay.example.com` and the access token, then save.
   The status shows *Reachable from the Internet* once registered.
2. Connect the iPad once on the local network. The next session records the relay.
3. Away from home, open Vision-Pro on the iPad: the PC appears with **Via Internet**.

The cached iPad app keeps the PC's Content-Security-Policy, which already allows the configured relay.
After changing the relay address, open the app once on the local network.

## Tests

- `cargo test -p vp-relay` — STUN codec (RFC 5769 vectors), registration, ticket splice, TURN over
  UDP/TCP, peer restriction, WebSocket media fallback.
- `cargo test -p vp-agent --test relay_link` — the real agent against the real relay: token refusal,
  registration, automatic reconnection, Local Only shutdown.
- `packages/protocol/test/e2e-relay.ts` — code pairing and authentication through a running relay
  and agent.
