# Vision-Pro

Vision-Pro transforms an iPad into a secure, low-latency control surface for a Windows PC. The local path is direct: the Windows agent handles authenticated signaling, screen capture, encoding, WebRTC media, and validated input without routing the video through an account server.

The product currently contains a functional iPad PWA, a native Windows/Tauri control application backed by Rust, a shared binary and signaling protocol, and an optional loopback account service.

## Project map

| Path | Responsibility |
| --- | --- |
| `apps/ipad` | Safari/iPadOS PWA, pairing, WebRTC receiver, touch/trackpad/Pencil input, creative profiles, quality controls, statistics |
| `apps/desktop` | Native Tauri interface for service, devices, capture, streaming, input, security, network and diagnostics |
| `agent/crates/vp-agent` | Rust agent, mDNS discovery, authenticated WebSocket signaling, pairing, session lifecycle, WebRTC, Windows capture/encode/input, USB Direct cable detection, Remote Access relay client |
| `agent/crates/vp-relay` | Internet relay: challenge-signed agent registry, end-to-end encrypted signaling splice, TURN restricted to registered PCs |
| `packages/protocol` | Shared TypeScript wire protocol, cryptography, strict validation, input encoding and application profiles |
| `packages/design` | Shared React components and icons |
| `server` | Optional private account service and static PWA host; never transports the local video stream |
| `design-system/vision-pro` | Canonical visual and interaction rules |
| `docs` | Architecture, security and operational notes |

## Requirements

- Windows 10 or 11 for the native agent
- Node.js 22.12 or newer
- Rust stable with the MSVC target
- Microsoft C++ Build Tools and WebView2 for native Windows builds
- iPadOS Safari for the target client experience

## Development

```powershell
npm install
npm run dev:ipad
npm run dev:desktop
```

Run the headless agent for local development:

```powershell
cargo run --manifest-path agent/Cargo.toml --bin vp-agentd
```

The secure default binds the agent on the local network at HTTPS port `47800`, advertises `_vision-pro._tcp.local`, and still requires explicit pairing. The special loopback development endpoint is off by default and must never be enabled as a production exposure mechanism.

## Validation and builds

```powershell
npm run typecheck
npm test
npm run build:web
cargo test --manifest-path agent/Cargo.toml --workspace
npm run test:e2e:usb -w @vision-pro/protocol
npm run tauri --workspace @vision-pro/desktop -- build
```

## USB Direct

Plug the iPad (or an Android tablet) into the PC with a cable: Vision-Pro detects it instantly and the whole session — signaling, video, cursor and input — goes through the cable, with no Wi-Fi or router. A Wi-Fi session in progress moves onto the cable when it is plugged in, and falls back to Wi-Fi within about a second when it is unplugged, without a black frame.

The cable carries IP from the tablet's USB network function: Personal Hotspot on an iPad (Wi-Fi + Cellular models, with Apple's "Apple Devices" drivers on Windows) or USB tethering on Android. The PC app's **Network → USB cable** section shows the detected tablet, what to enable on it, and a one-click (UAC) fix when Windows blocks the cable network. See [USB Direct](docs/USB.md).

## Remote Access relay

To use the iPad from another network, run `vp-relay` on a public host with a TLS certificate, then enter its address in **Network → Remote Access** on the PC. Pair the iPad once on the local network; it then shows the PC as **Via Internet** when away. Signaling stays end-to-end encrypted and pinned to the PC key, and media stays DTLS-SRTP. See [relay](docs/RELAY.md) for the protocol, threat model and deployment.

```powershell
cargo run --manifest-path agent/Cargo.toml -p vp-relay -- --host relay.example.com --cert fullchain.pem --key privkey.pem --token <secret>
```

The Windows installer is emitted under `apps/desktop/src-tauri/target/release/bundle/nsis`. The standalone binary is under `apps/desktop/src-tauri/target/release`.

## Optional private account service

The account service binds to `127.0.0.1` and is intended to sit behind a TLS reverse proxy on a private deployment. It is not required for direct LAN control.

```powershell
$env:VP_ACCOUNT_PORT = '3000'
$env:VP_ACCOUNT_DATA = 'D:\Vision-Pro\users.json'
$env:VP_WEB_ROOT = "$PWD\apps\ipad\dist"
$env:VP_ALLOW_REGISTRATION = '0'
npm run start --workspace @vision-pro/server
```

Registration is disabled unless `VP_ALLOW_REGISTRATION=1`. `VP_INSECURE_DEV=1` removes the Secure cookie requirement only for an explicit loopback HTTP development test; never use it in deployment.

## Current scope boundaries

LAN discovery, approval, secure pairing, direct WebRTC streaming, USB cable streaming and remote input are implemented. Remote Access through a self-operated `vp-relay` is implemented and stays disabled until the PC is switched to Remote Access with a relay address. The optional account service currently supports hardened password sessions; WebAuthn/passkeys, 2FA and preference synchronization remain capability-gated and are not advertised as active features.

See [architecture](docs/ARCHITECTURE.md) and [security model](docs/SECURITY.md) for the trust boundaries and limitations.

