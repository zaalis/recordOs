# Vision-Pro architecture

The iPad is a drawing tablet and trackpad for the PC: nothing is captured or streamed from the PC.
A session is one end-to-end encrypted WebSocket that carries the handshake, then the iPad's input
and the PC's state updates (displays, active application, permission).

## Local data path

```text
iPad PWA (https://<PC address>:47800)
  | WSS /ws: TLS 1.3, then the Vision-Pro handshake and an AES-256-GCM session channel
  |   iPad → PC: input events (`in` reliable, `rt` latest-wins hover moves), pings, display choice
  |   PC → iPad: session.ready, displays, active application, permission changes
  v
Windows Rust agent
  | strict binary decoding, per-category permission, rate limit
  | Windows input and pen injection APIs (SendInput, synthetic pen and touch devices)
  v
Windows desktop (the selected display)
```

The optional account server is outside this path. Local input is not proxied through it.

## Pairing

```text
PC: "Connect Device"  →  shows a 6-digit code and the address to open, e.g. http://192.168.1.20:47801
iPad: opens that address → setup page: installs the PC's trust profile once, then opens the app
iPad: enters the code   → handshake with an HMAC(code, transcript) proof → paired, session starts
```

The setup page (plain HTTP, only while the window is open) sends the iPad to the HTTPS app on the
same address it was reached by. The code is single use: it is replaced after a successful pairing
and after five wrong attempts.

## USB cable path

```text
iPad ══ USB cable (Personal Hotspot / USB tethering: a private point-to-point IP link) ══ PC
  wss://<cable address>:47800/ws   the whole session
```

The agent detects tablets and cable interfaces (device and address notifications), keeps the
certificate and mDNS announcement valid for the cable address, marks sessions arriving on it as
USB sessions, tells Wi-Fi sessions when a cable appears and ends cable sessions when it goes. The
iPad probes cable endpoints before connecting and moves a running session onto the cable. See
[USB Direct](USB.md).

## From another network

```text
Tailscale (recommended):  iPad ── WireGuard (Tailscale) ── PC     https://100.x.y.z:47800, same as the LAN
Relay (self-hosted):      iPad ── wss ── vp-relay ── wss (one-time ticket) ── PC
```

With Tailscale on both devices the PC's Tailscale address behaves like a LAN address: it is in the
certificate, accepted in Local Only mode while the PC is on a Tailscale network, and shown by
"Connect Device". The relay splices a session that remains end-to-end encrypted. See
[relay](RELAY.md).

## Runtime layers

1. The agent creates or loads its device identity and local certificate, opens the bounded session
   service, and advertises discovery metadata over mDNS.
2. The PWA opens on the PC's address and pairs with the code shown on the PC. Discovery supplies an
   address; it does not confer trust.
3. Devices that must be confirmed again (untrusted, or "confirm every connection") show the same
   short authentication string on both screens and need **Allow** on the PC.
4. The agent starts an input injector for the session, mapped to the selected display.
5. Input messages are decoded against a fixed schema, bounded, permission-checked and dispatched to
   typed Windows APIs.
6. Revocation removes the stored authorization and terminates active sessions for that device.

## UI architecture

The iPad app separates persisted preferences, connection state and input translation. Touch,
trackpad and Pencil modes share a binary encoder but keep independent gesture semantics. The tablet
surface keeps the proportions of the PC display (an active area, like a graphics tablet's "keep
proportions" mode). Application profiles are data-driven and selected from the Windows foreground
executable when automatic profiles are enabled.

The Windows interface calls a narrow Tauri command facade. It does not expose a general shell or
arbitrary native invocation. The same `vp-agent` crate powers the visual application and the
headless development binary.

## Latency

- Input is encrypted and sent as soon as it happens; the PC injects it on a dedicated, prioritized
  thread and never waits for anything else.
- Hover moves are latest-wins: while the socket is backed up the iPad keeps only the newest one, and
  the PC drops a move older than the last one applied. Presses, releases and strokes are reliable
  and ordered; a pending hover move is always sent before the press that follows it.
- TCP_NODELAY on every session socket.
