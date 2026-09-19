# Vision-Pro architecture

## Local data path

```text
iPad PWA
  | authenticated WebSocket (signaling only)
  | DTLS/SRTP WebRTC (video)
  | DTLS data channels (validated input/control)
  v
Windows Rust agent
  | Windows Graphics Capture / Desktop Duplication
  | Media Foundation hardware encoder when available
  | Windows input and pen injection APIs
  v
Windows desktop
```

The optional account server is outside this path. Local video and input are not proxied through it.

## USB cable path

```text
iPad ══ USB cable (Personal Hotspot / USB tethering: a private point-to-point IP link) ══ PC
  wss://<cable address>:47800/ws   signaling
  DTLS/SRTP on the cable address   video, cursor, input (the only ICE candidate offered)
```

The agent detects tablets and cable interfaces (device and address notifications), keeps the
certificate and mDNS announcement valid for the cable address, marks sessions arriving on it as
USB sessions, tells Wi-Fi sessions when a cable appears and ends cable sessions when it goes. The
iPad probes cable endpoints before connecting and moves a running session onto the cable. See
[USB Direct](USB.md).

## Remote Access path

```text
iPad ── wss signaling ─┐                 ┌─ wss data connection (one-time ticket) ── PC
     └─ TURN (udp/tcp/tls) ── vp-relay ── media tunnel (UDP, WebSocket fallback) ──┘
```

The relay splices signaling that remains end-to-end encrypted and routes DTLS/SRTP packets between an iPad TURN allocation and one virtual ICE candidate of the PC. See [relay](RELAY.md).

## Runtime layers

1. The agent creates or loads its device identity and local certificate, opens the bounded signaling service, and advertises discovery metadata over mDNS.
2. The PWA discovers or scans an agent and begins a time-limited pairing ceremony. Discovery supplies an address; it does not confer trust.
3. Both screens show the same short authentication string. The PC must approve an unknown device before a device key is persisted.
4. The agent negotiates the codec and constrained quality settings, creates the peer connection, and opens separated reliable-control and real-time pointer channels.
5. Capture frames flow directly to the peer. Input messages are decoded against a fixed schema, bounded, permission-checked and dispatched to typed Windows APIs.
6. Revocation removes the stored authorization and terminates active sessions for that device.

## UI architecture

The iPad app separates persisted preferences, connection state, input translation and viewport transforms. Touch, trackpad and Pencil modes share a binary encoder but keep independent gesture semantics. Application profiles are data-driven and selected from the Windows foreground executable when automatic profiles are enabled.

The Windows interface calls a narrow Tauri command facade. It does not expose a general shell or arbitrary native invocation. The same `vp-agent` crate powers the visual application and the headless development binary.

## Performance strategy

- Capture and encoding stay in native Rust/Windows code.
- Hardware codecs are preferred and negotiated against browser capability.
- Pointer movement uses an unordered low-latency channel; state-changing commands use a reliable channel.
- Encoded video has its own two-frame queue (stale frames are dropped, never delayed); cursor and control events use a separate queue so mouse movement never displaces video.
- Hardware encoder output is collected as soon as it completes (polled while a frame is in flight), and a frame the encoder holds back is flushed within one frame interval instead of waiting for the next screen change.
- Input is dispatched immediately and does not wait for a video frame.
- Quality requests are bounded by both protocol validation and agent policy.

