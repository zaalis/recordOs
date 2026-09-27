# Vision-Pro security model (macOS)

## Trust boundaries

- A discovered device is untrusted.
- A paired key is authorized only for its recorded permission level.
- A trusted device may reconnect quickly, but session credentials remain temporary and revocable.
- Remote Access is disabled by default and is not considered operational without a separately configured strong-authentication relay.

## Implemented controls

- Local-first networking and no unnecessary cloud video path
- Time-limited QR/code pairing with explicit approval on the Mac and matching short authentication string
- Public-key device identity, signed challenges, ephemeral session material and replay tracking
- DTLS/SRTP WebRTC transport and typed data channels
- Strict message shape, size, enum, coordinate, bitrate, resolution and FPS validation
- Per-device view/control authorization, trust expiry, attempt lockout, audit history and immediate revocation
- No browser-accessible arbitrary shell command
- Emergency global shortcut: ⌘⌥⇧⌫ (Command-Option-Shift-Delete)
- Screen capture and input injection gated by the user's macOS Screen Recording and Accessibility permissions
- Optional account passwords hashed with Argon2id
- Secure, HttpOnly, SameSite=Strict session cookies; CSRF token and same-origin checks
- CSP, clickjacking, MIME-sniffing, referrer and permissions headers
- Bounded request bodies and account rate limiting

## Secrets and local state

Agent state is stored below `~/Library/Application Support/Vision-Pro` (directory 0700, files 0600). The agent identity and local authority are sealed with AES-256-GCM under a master key held only in the user's login Keychain (`com.vision-pro.agent`), so copying the directory to another account or Mac does not expose them. A locked or refused Keychain stops the agent instead of silently replacing its identity. Do not publish this directory, account data, private keys, QR secrets, logs containing identifiers, or `.env` files. Production account data should live outside the source checkout with filesystem permissions limited to the service identity.

## Deployment requirements

- Keep Local Only unless remote access has been deliberately engineered and reviewed.
- Terminate HTTPS at a trusted reverse proxy for the optional account server.
- Keep registration disabled except during a controlled enrollment window.
- Do not use `VP_INSECURE_DEV=1` outside loopback testing.
- If the macOS application firewall is enabled, allow incoming connections for Vision-Pro only; keep stealth mode on for other services.
- Sign the app with a Developer ID, enable the Hardened Runtime and notarise it before distributing broadly; protect updates with a signed manifest.

## Known capability gates

WebAuthn/passkeys, optional 2FA, an Internet relay, automated signed updates and third-party penetration testing are not complete in this repository. The UI and configuration do not claim those capabilities are active. They require deployment-specific origins, credentials, signing infrastructure and independent security validation.

Security issues should be handled privately; do not include live credentials, private keys or personal device identifiers in a public report.

