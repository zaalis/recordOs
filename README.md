# Record OS

Record OS turns an iPad into a secure, low-latency graphics tablet and trackpad for a Windows PC: Apple Pencil (pressure, tilt, hover), touch, gestures, keyboard and creative-app shortcuts. Nothing is captured or streamed from the PC — you look at the PC's own screen. The iPad's input travels through one end-to-end encrypted connection to the Windows agent, which validates it and injects it.

The product contains an iPad PWA, a native Windows/Tauri control application backed by Rust, a shared binary and session protocol, and an optional loopback account service.

## Project map

| Path | Responsibility |
| --- | --- |
| `apps/ipad` | Safari/iPadOS PWA, code pairing, tablet surface with touch/trackpad/Pencil input, creative profiles |
| `apps/desktop` | Native Tauri interface for service, devices, displays, input, security, network and diagnostics |
| `agent/crates/vp-agent` | Rust agent, mDNS discovery, authenticated encrypted sessions, code pairing, Windows input injection, USB Direct cable detection, Tailscale addresses, Remote Access relay client |
| `agent/crates/vp-relay` | Internet relay: challenge-signed agent registry, splice of end-to-end encrypted sessions |
| `packages/protocol` | Shared TypeScript wire protocol, cryptography, strict validation, input encoding and application profiles |
| `packages/design` | Shared React components and icons |
| `server` | Optional private account service and static PWA host; never transports the local session |
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

The secure default binds the agent on the local network at HTTPS port `47800`, advertises `_vision-pro._tcp.local`, and still requires explicit pairing. The special loopback development endpoint is off by default and must never be enabled as a production exposure mechanism. `--connect` opens the pairing window and prints the code and the address to open.

## Connecting an iPad

1. On the PC: **Connect Device** (Connecter un appareil). Record OS shows a 6-digit code and an address such as `http://192.168.1.20:47801`.
2. On the iPad, type that address in Safari. The first time, the page installs the PC's trust profile (Settings → Profile Downloaded → Install, then Settings → General → About → Certificate Trust Settings), then opens Record OS by itself.
3. Enter the code. The iPad is paired and the session starts; later it reconnects with **Connect** alone.

The code is the authorization: it is valid for five minutes, allows five attempts and is replaced as soon as it has paired a device.

## From another network

The public deployment at https://vision-pro.34-73-186-88.sslip.io supports first pairing over the Internet. Configure that hostname in the PC's **Network → Remote Access**, then on the iPad choose **Autre ordinateur** and enter the PC's Agent ID, full Internet fingerprint from **About**, and the code shown by **Connect Device**. Later connections use the saved PC key and relay. See [deployment and usage](docs/DEPLOYMENT-2026-10-22.md).

- **Tailscale (recommended).** Install [Tailscale](https://tailscale.com) on the PC and the iPad with the same account. The PC's Tailscale address (`100.x.y.z`) is accepted like a local one and **Connect Device** shows it too: open that address on the iPad once (profile + code) and it works from anywhere — no server, no open port, encrypted WireGuard tunnel plus Record OS's own end-to-end encryption. Use the Tailscale address at home as well: the app, its pairing and its preferences belong to the address it was opened from.
- **Self-hosted relay.** Run `vp-relay` on a public host and enter it in **Network → Remote Access** (see below).

## Validation and builds

```powershell
npm run typecheck
npm test
npm run build:web
cargo test --manifest-path agent/Cargo.toml --workspace
node --experimental-strip-types packages/protocol/test/e2e-agent.ts
npm run test:e2e:usb -w @vision-pro/protocol
npm run tauri --workspace @vision-pro/desktop -- build
```

The end-to-end tests start their own debug `vp-agentd` with an isolated data directory (`cargo build --manifest-path agent/Cargo.toml -p vp-agent` first).

## USB Direct

Plug the iPad (or an Android tablet) into the PC with a cable: Record OS detects it instantly and the whole session goes through the cable, with no Wi-Fi or router. A Wi-Fi session in progress moves onto the cable when it is plugged in, and falls back to Wi-Fi within about a second when it is unplugged.

The cable carries IP from the tablet's USB network function: Personal Hotspot on an iPad (Wi-Fi + Cellular models, with Apple's "Apple Devices" drivers on Windows) or USB tethering on Android. The PC app's **Network → USB cable** section shows the detected tablet, what to enable on it, and a one-click (UAC) fix when Windows blocks the cable network. See [USB Direct](docs/USB.md).

## Remote Access relay

To use the iPad from another network without Tailscale, run `vp-relay` on a public host with a TLS certificate, then enter its address in **Network → Remote Access** on the PC. Pair the iPad once on the local network; it then shows the PC as **Via Internet** when away. The session, input included, stays end-to-end encrypted and pinned to the PC key. See [relay](docs/RELAY.md) for the protocol, threat model and deployment.

```powershell
cargo run --manifest-path agent/Cargo.toml -p vp-relay -- --host relay.example.com --cert fullchain.pem --key privkey.pem --token <secret>
```

The Windows installer is emitted under `apps/desktop/src-tauri/target/release/bundle/nsis`. The standalone binary is under `apps/desktop/src-tauri/target/release`.

## Optional private account service

The account service binds to `127.0.0.1` and is intended to sit behind a TLS reverse proxy on a private deployment. It is not required for direct LAN control.

```powershell
$env:VP_ACCOUNT_PORT = '3000'
$env:VP_ACCOUNT_DATA = 'D:\Record OS\users.json'
$env:VP_WEB_ROOT = "$PWD\apps\ipad\dist"
$env:VP_ALLOW_REGISTRATION = '0'
npm run start --workspace @vision-pro/server
```

Registration is disabled unless `VP_ALLOW_REGISTRATION=1`. `VP_INSECURE_DEV=1` removes the Secure cookie requirement only for an explicit loopback HTTP development test; never use it in deployment.

Account storage supports one server process per data file. Email addresses are normalized (trimmed and lowercased) and unique, as are account IDs; this does not verify ownership of an email address or prevent a person from using several addresses. Concurrent inserts are serialized and committed to memory only after the atomic file replacement succeeds. An unreadable, malformed or duplicated account file stops startup instead of silently resetting accounts.

Behind a loopback TLS reverse proxy, set `VP_TRUST_PROXY=1` only when the proxy overwrites `X-Forwarded-For` with the actual client IP. This preserves per-client throttling. Password hashing and verification are limited to two concurrent jobs to bound memory usage. Production POST requests require an HTTPS origin matching the forwarded Host.

## Current scope boundaries

LAN discovery, code pairing, encrypted remote input over the local network, Tailscale and USB cable are implemented. Remote Access through a self-operated `vp-relay` is implemented and stays disabled until the PC is switched to Remote Access with a relay address. The optional account service currently supports hardened password sessions; WebAuthn/passkeys, 2FA and preference synchronization remain capability-gated and are not advertised as active features.

See [architecture](docs/ARCHITECTURE.md) and [security model](docs/SECURITY.md) for the trust boundaries and limitations.

