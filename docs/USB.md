# USB Direct

Plug the tablet into the Mac with a cable (USB-C, or Lightning to USB-A) and the whole session —
signaling, video, cursor and input — travels over that cable. No router or Wi-Fi network is
needed. Sessions keep the same protocol, the same end-to-end encryption and the same pairing.

## How it works

A web app cannot open a raw USB pipe, so the cable carries IP. The tablet exposes a USB network
function, and Vision-Pro treats that interface as a private, point-to-point link:

| Tablet | USB network function | What to enable on the tablet |
| --- | --- | --- |
| iPad (Wi-Fi + Cellular) | Personal Hotspot over USB (`iPad USB` in System Settings → Network) | Settings → Personal Hotspot → Allow Others to Join, then trust the Mac |
| Android tablet | USB tethering (NCM) | Settings → Connections → Mobile Hotspot and Tethering → USB tethering |

macOS ships the drivers for an iPad's USB network and for NCM tethering (recent Android devices);
nothing has to be installed. The older RNDIS tethering of some Android devices is not supported by
macOS.

```text
iPad ══ USB cable ══ Mac            (172.20.10.1 ⇄ 172.20.10.2, private /28 created by the iPad)
  wss://172.20.10.2:47800/ws  signaling (TLS 1.3 + end-to-end encrypted channel)
  DTLS/SRTP on 172.20.10.2    video, cursor and input (only ICE candidate offered)
```

### On the Mac (`agent/crates/vp-agent/src/usb.rs`, `platform/usb.rs`)

1. A watcher compares the USB bus (IOKit registry ids) and the IPv4 addresses every 0.7 s and wakes
   the monitor on any change, with a 3 s rescan as fallback.
2. It lists present USB devices (`IOUSBHostDevice`) and recognises iPads/iPhones (Apple VID `05AC`,
   iOS product range) and Android devices (ADB, RNDIS or NCM functions only — storage, cameras
   and hubs never match).
3. It lists network interfaces whose IOKit ancestry is a USB device and that carry a private IPv4
   address: these are cable links (`en7 · iPad USB`).
4. On any address change it reissues the TLS certificate (the cable address is in its SAN) and
   re-announces Bonjour with the new addresses — before any iPad is told about the cable.
5. A session whose TCP connection arrived on a cable address is a **USB session**: the WebRTC
   answer offers the cable address as its only ICE candidate, so media cannot take another path.
   Local-network sessions never offer cable addresses.
6. When a cable link appears, every local-network session receives `link.usb` with the cable
   endpoints. When it disappears, its sessions end at once with reason `link`.

### On the iPad (`apps/ipad/src/lib/connection.ts`)

- Before each connection the app asks the Mac for its current cable endpoints (`/api/info`
  `usb.urls`) and probes them: a WebSocket that must answer with the pinned Mac's `hello`. Only the
  tablet at the other end of the cable can reach that address, so a successful probe means "this
  iPad is the one plugged in". The session then opens on the cable; otherwise on Wi-Fi.
- `link.usb` during a Wi-Fi session: the app probes the cable and moves the whole session onto it.
  The last picture stays on screen until the first cable frame (measured: no black frame, live
  video again after about 1 s).
- Cable unplugged: the session end (`link`), a closed socket or an ICE disconnection (1.2 s grace
  on a cable, 4 s on Wi-Fi instead of the browser's ~30 s) reconnects immediately, preferring
  Wi-Fi for 15 s. Again the picture stays; a toast says what happened.
- Without Wi-Fi at all the service worker serves the installed app after 2.5 s even though the
  `.local` name does not resolve, and the home screen finds the Mac through the cable endpoints it
  learned earlier (plus the iPad hotspot defaults `172.20.10.2–5`). The Mac then shows as
  **Connected by USB cable**.

## macOS network integration

macOS has no Public/Private network profiles, so a cable network needs no change. If the macOS
application firewall is on, it asks once to allow incoming connections for Vision-Pro; that choice
applies to every network, the cable included.

When the iPad's hotspot becomes the Mac's primary route, the Mac's Internet traffic uses the
tablet's mobile data. The Mac app warns about it; fix it once in System Settings → Network → ⋯ →
Set Service Order by placing Wi-Fi above `iPad USB`. The stream stays on the cable either way.

## Settings

Network → USB cable → **Stream over USB cable** (`network.usbDirect`, on by default). When off,
nothing is scanned and a session arriving on a cable address is handled like any local session.

## Limits

- A Wi-Fi-only iPad has no Personal Hotspot, hence no USB network function a web app could use.
  Streaming over its cable would require a native iPad app talking to the Mac through Apple's
  usbmux service; that is outside this web-app architecture.
- The app origin is `https://vp-xxxxxxxx.local:47800`. Pair and open the app once while the iPad
  can resolve that name (Wi-Fi, or the cable if the iPad resolves `.local` names over its hotspot);
  afterwards the installed app starts and connects over the cable alone.

## Testing without a tablet

Debug builds of `vp-agentd` simulate a cable on the loopback address `127.0.0.2`. macOS only
answers on `127.0.0.1` by default, so add the alias once:

```bash
sudo ifconfig lo0 alias 127.0.0.2 up
```

```bash
cargo build --manifest-path agent/Cargo.toml -p vp-agent
npm run test:e2e:usb -w @vision-pro/protocol
```

The test starts its own agent (isolated `VISION_PRO_HOME`) and checks the session path, the probe,
the single cable ICE candidate, `link.usb` on plug-in and the immediate `link` end on unplug.
Interactively: `vp-agentd --dev-loopback --simulate-usb`, then type `u` to unplug/plug the cable
while the iPad app (`npm run dev:ipad`) streams.
