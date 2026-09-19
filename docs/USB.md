# USB Direct

Plug the tablet into the PC with a cable (USB-C, or Lightning to USB-A) and the whole session —
signaling, video, cursor and input — travels over that cable. No router or Wi-Fi network is
needed. Sessions keep the same protocol, the same end-to-end encryption and the same pairing.

## How it works

A web app cannot open a raw USB pipe, so the cable carries IP. The tablet exposes a USB network
function, and Vision-Pro treats that interface as a private, point-to-point link:

| Tablet | USB network function | What to enable on the tablet |
| --- | --- | --- |
| iPad (Wi-Fi + Cellular) | Personal Hotspot over USB (`Apple Mobile Device Ethernet`) | Settings → Personal Hotspot → Allow Others to Join, then trust the PC |
| Android tablet | USB tethering (RNDIS or NCM) | Settings → Connections → Mobile Hotspot and Tethering → USB tethering |

On Windows the iPad needs Apple's USB drivers: install **Apple Devices** from the Microsoft Store
(or iTunes). The PC app says so when it sees an iPad whose USB functions have no driver.

```text
iPad ══ USB cable ══ PC            (172.20.10.1 ⇄ 172.20.10.2, private /28 created by the iPad)
  wss://172.20.10.2:47800/ws  signaling (TLS 1.3 + end-to-end encrypted channel)
  DTLS/SRTP on 172.20.10.2    video, cursor and input (only ICE candidate offered)
```

### On the PC (`agent/crates/vp-agent/src/usb.rs`, `platform/usb.rs`)

1. A monitor thread wakes on USB arrivals/removals (`CM_Register_Notification`) and on address
   changes (`NotifyUnicastIpAddressChange`), with a 3 s rescan as fallback.
2. It lists present USB devices (SetupAPI) and recognises iPads/iPhones (Apple VID `05AC`, iOS
   product range) and Android devices (ADB, MTP, RNDIS or NCM functions only — storage, cameras
   and hubs never match).
3. It lists network interfaces whose `PnPInstanceId` is a USB function and that carry a private
   IPv4 address: these are cable links.
4. On any address change it reissues the TLS certificate (the cable address is in its SAN) and
   re-announces mDNS with the new addresses — before any iPad is told about the cable.
5. A session whose TCP connection arrived on a cable address is a **USB session**: the WebRTC
   answer offers the cable address as its only ICE candidate, so media cannot take another path.
   Local-network sessions never offer cable addresses.
6. When a cable link appears, every local-network session receives `link.usb` with the cable
   endpoints. When it disappears, its sessions end at once with reason `link`.

### On the iPad (`apps/ipad/src/lib/connection.ts`)

- Before each connection the app asks the PC for its current cable endpoints (`/api/info`
  `usb.urls`) and probes them: a WebSocket that must answer with the pinned PC's `hello`. Only the
  tablet at the other end of the cable can reach that address, so a successful probe means "this
  iPad is the one plugged in". The session then opens on the cable; otherwise on Wi-Fi.
- `link.usb` during a Wi-Fi session: the app probes the cable and moves the whole session onto it.
  The last picture stays on screen until the first cable frame (measured: no black frame, live
  video again after about 1 s).
- Cable unplugged: the session end (`link`), a closed socket or an ICE disconnection (1.2 s grace
  on a cable, 4 s on Wi-Fi instead of the browser's ~30 s) reconnects immediately, preferring
  Wi-Fi for 15 s. Again the picture stays; a toast says what happened.
- Without Wi-Fi at all the service worker serves the installed app after 2.5 s even though the
  `.local` name does not resolve, and the home screen finds the PC through the cable endpoints it
  learned earlier (plus the iPad hotspot defaults `172.20.10.2–5`). The PC then shows as
  **Connected by USB cable**.

## Windows network integration

A tethering interface is a new network that Windows usually classifies as **Public**. If the
firewall only allows Vision-Pro on private networks, the tablet cannot reach the PC through the
cable. The PC app reads the network category (Network List Manager) and the firewall rules
(`INetFwPolicy2`) and, when needed, offers **Allow**: with UAC consent it

- marks the cable network as private,
- adds an inbound rule for the Vision-Pro program restricted to that interface,
- raises the interface's route metric so Windows keeps using Wi-Fi/Ethernet for the Internet
  instead of the tablet's mobile data (offered as **Fix** when the PC's Internet goes through the
  tablet).

## Settings

Network → USB cable → **Stream over USB cable** (`network.usbDirect`, on by default). When off,
nothing is scanned and a session arriving on a cable address is handled like any local session.

## Limits

- A Wi-Fi-only iPad has no Personal Hotspot, hence no USB network function a web app could use.
  Streaming over its cable would require a native iPad app talking to the PC through Apple's
  usbmux service; that is outside this web-app architecture.
- The app origin is `https://vp-xxxxxxxx.local:47800`. Pair and open the app once while the iPad
  can resolve that name (Wi-Fi, or the cable if the iPad resolves `.local` names over its hotspot);
  afterwards the installed app starts and connects over the cable alone.

## Testing without a tablet

Debug builds of `vp-agentd` simulate a cable on the loopback address `127.0.0.2`:

```powershell
cargo build --manifest-path agent/Cargo.toml -p vp-agent
npm run test:e2e:usb -w @vision-pro/protocol
```

The test starts its own agent (isolated `VISION_PRO_HOME`) and checks the session path, the probe,
the single cable ICE candidate, `link.usb` on plug-in and the immediate `link` end on unplug.
Interactively: `vp-agentd --dev-loopback --simulate-usb`, then type `u` to unplug/plug the cable
while the iPad app (`npm run dev:ipad`) streams.
