# Vision-Pro security model

## Trust boundaries

- A discovered device is untrusted.
- A paired key is authorized only for its recorded permission level.
- A trusted device may reconnect quickly, but session credentials remain temporary and revocable.
- Remote Access is disabled by default. When enabled, the PC only makes an outbound, signed registration to the configured relay; the relay is untrusted for confidentiality and integrity (see [relay](RELAY.md)).

## Implemented controls

- Local-first networking and no unnecessary cloud video path
- Time-limited QR/code pairing with explicit PC approval and matching short authentication string
- Public-key device identity, signed challenges, ephemeral session material and replay tracking
- DTLS/SRTP WebRTC transport and typed data channels
- Strict message shape, size, enum, coordinate, bitrate, resolution and FPS validation
- Per-device view/control authorization, trust expiry, attempt lockout, audit history and immediate revocation
- No browser-accessible arbitrary shell command
- Emergency global shortcut: `Ctrl+Alt+Shift+Backspace`
- Optional account passwords hashed with Argon2id
- Secure, HttpOnly, SameSite=Strict session cookies; CSRF token and same-origin checks
- CSP, clickjacking, MIME-sniffing, referrer and permissions headers
- Bounded request bodies and account rate limiting
- Relay: challenge-signed agent registration, optional access token, one-time accept tickets, per-source rate limits
- Relay: TURN credentials delivered only inside the encrypted channel and restricted to the PC's leased virtual address; code-less pairing refused over the Internet
- USB Direct: cable endpoints are announced only inside the encrypted channel or on the local `/api/info`; the iPad accepts only `wss://` to private IPv4 addresses and probes the pinned PC identity before use; the same authentication, pairing and DTLS-SRTP apply over the cable
- USB Direct: the Windows network/firewall change for a cable interface requires explicit UAC consent and is scoped to the Vision-Pro program on that interface

## Secrets and local state

Agent state is stored below `%APPDATA%\Vision-Pro`. Do not publish this directory, account data, private keys, QR secrets, logs containing identifiers, or `.env` files. Production account data should live outside the source checkout with filesystem permissions limited to the service identity.

## Deployment requirements

- Keep Local Only unless remote access has been deliberately engineered and reviewed.
- Terminate HTTPS at a trusted reverse proxy for the optional account server.
- Keep registration disabled except during a controlled enrollment window.
- Do not use `VP_INSECURE_DEV=1` outside loopback testing.
- Restrict the Windows firewall rule to the private network profile and configured agent port. A USB cable network is made private (with consent) rather than opening the public profile.
- The web app's Content Security Policy allows `wss://*:<agent port>` so the installed app (whose origin is the `.local` name) can reach the PC through a cable address; scripts remain restricted to `'self'`.
- Protect software updates with code signing and a signed update manifest before distributing broadly.

## Known capability gates

WebAuthn/passkeys, optional 2FA, automated signed updates and third-party penetration testing are not complete in this repository. The relay requires an operator-provided public host and certificate. The UI and configuration do not claim those capabilities are active. They require deployment-specific origins, credentials, signing infrastructure and independent security validation.

Security issues should be handled privately; do not include live credentials, private keys or personal device identifiers in a public report.

