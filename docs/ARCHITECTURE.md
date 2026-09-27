# Vision-Pro architecture (macOS)

## Local data path

```text
iPad PWA
  | authenticated WebSocket (signaling only)
  | DTLS/SRTP WebRTC (video)
  | DTLS data channels (validated input/control)
  v
macOS Rust agent  ──  Objective-C bridge (agent/crates/vp-agent/native)
  | ScreenCaptureKit (IOSurface frames, scaled + NV12 by the WindowServer)
  | VideoToolbox hardware H.264 / HEVC (OpenH264 software fallback)
  | Quartz Event Services: mouse, keyboard, scroll, tablet pen, magnify
  v
macOS desktop
```

The optional account server is outside this path. Local video and input are not proxied through it.

## Runtime layers

1. The agent creates or loads its device identity and local certificate (sealed with a key kept in the login Keychain), opens the bounded signaling service, and advertises discovery metadata over Bonjour (`_vision-pro._tcp`).
2. The PWA discovers or scans an agent and begins a time-limited pairing ceremony. Discovery supplies an address; it does not confer trust.
3. Both screens show the same short authentication string. The Mac must approve an unknown device before a device key is persisted.
4. The agent negotiates the codec and constrained quality settings, creates the peer connection, and opens separated reliable-control and real-time pointer channels.
5. Capture frames flow directly to the encoder without CPU copies and then to the peer. Input messages are decoded against a fixed schema, bounded, permission-checked, translated to macOS keys and posted with Quartz Event Services.
6. Revocation removes the stored authorization and terminates active sessions for that device.

## Native bridge

`native/vp_macos.m` is compiled by `build.rs` with ARC for macOS 13+. It exposes a narrow C ABI (`native/vp_macos.h`): displays, privacy permissions, capture, encoder, cursor, input injection, frontmost application, power assertions and Keychain items. Rust wraps it in `src/platform/*` with owned handles (`Frame`, `ScreenCapture`, `VtEncoder`) that release their Objective-C objects on drop.

- Capture excludes the pointer; its position (every loop, ~120 Hz) and its shape (`NSCursor.currentSystemCursor`, 10 Hz with hashing) are sent on the real-time channel so the iPad draws it locally.
- The encoder runs VideoToolbox in real-time mode, low-latency rate control when available, no frame reordering, a quarter-second data-rate window and forced IDR on demand. Output is converted from AVCC/HVCC to Annex B with parameter sets on every keyframe.
- Input coordinates are normalised on the iPad and mapped to global display points on the Mac. Drags use the proper `*MouseDragged` events, double clicks use the click-state field, and modifier state is attached to every event.
- The Apple Pencil becomes a tablet stylus: proximity events (pen or eraser) plus tablet-point mouse events carrying pressure, tilt and rotation, which Photoshop, Blender, Krita and other apps read as native pen input.
- Pinch is forwarded as a native magnify gesture; 3D applications instead receive wheel notches, matching their navigation models.

## Privacy permissions

ScreenCaptureKit requires **Screen Recording**; posting input requires **Accessibility**; Bonjour and LAN sockets trigger the **Local Network** prompt (declared in `Info.plist`). The desktop app reports these states and opens the right System Settings pane; the headless agent prints them at start-up.

## UI architecture

The iPad app separates persisted preferences, connection state, input translation and viewport transforms. Touch, trackpad and Pencil modes share a binary encoder but keep independent gesture semantics. Application profiles are data-driven and selected from the bundle identifier of the frontmost Mac application when automatic profiles are enabled.

The Mac interface calls a narrow Tauri command facade. It does not expose a general shell or arbitrary native invocation. Closing the window keeps the service running (Dock icon reopens it, ⌘Q quits). The same `vp-agent` crate powers the visual application and the headless development binary.

## Performance strategy

- Capture, scaling, colour conversion and encoding stay on the GPU / media engine.
- Hardware codecs are preferred and negotiated against browser capability.
- Pointer movement uses an unordered low-latency channel; state-changing commands use a reliable channel.
- Input is dispatched immediately and does not wait for a video frame.
- Quality requests are bounded by both protocol validation and agent policy.
