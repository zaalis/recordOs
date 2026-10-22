export const PRODUCT_NAME = 'Vision-Pro';
export const PROTOCOL_VERSION = 1;

/** Default local ports. Everything stays on the LAN (or Tailscale) unless Remote Access is enabled on the PC. */
export const PORTS = {
  /** HTTPS + WSS: serves the web app and the session channel. */
  https: 47800,
  /** Plain HTTP setup page. Only open while "Connect Device" is showing on the PC. Serves the trust profile. */
  bootstrap: 47801,
  /** Loopback-only developer endpoint (disabled by default). */
  devLoopback: 47802,
} as const;

export const LIMITS = {
  /** Maximum size of any signaling frame, before or after decryption. */
  maxSignalFrameBytes: 64 * 1024,
  /** Maximum UTF-16 code units for a single TEXT input event. */
  maxTextUnits: 256,
  /** Maximum keys in a single COMBO input event. */
  maxComboKeys: 6,
  /** Maximum simultaneous touch contacts forwarded to Windows. */
  maxTouchContacts: 10,
  /** Maximum device name length, in characters. */
  maxDeviceName: 48,
  /** Messages per encryption epoch before a rekey is forced. */
  rekeyAfterMessages: 50_000,
  /** Time before a rekey is forced. */
  rekeyAfterMs: 10 * 60 * 1000,
} as const;

/** Domain-separation labels. Any change here is a protocol version bump. */
export const LABEL = {
  transcript: 'Vision-Pro/1 transcript',
  commit: 'Vision-Pro/1 commit',
  agentSig: 'Vision-Pro/1 agent',
  deviceSig: 'Vision-Pro/1 device',
  c2s: 'Vision-Pro/1 c2s',
  s2c: 'Vision-Pro/1 s2c',
  sas: 'Vision-Pro/1 sas',
  sessionId: 'Vision-Pro/1 session-id',
  frame: 'Vision-Pro/1 frame',
  rekey: 'Vision-Pro/1 rekey',
  pairSecret: 'Vision-Pro/1 pair-secret',
  rotate: 'Vision-Pro/1 rotate',
} as const;

export const DIRECTION = { c2s: 1, s2c: 2 } as const;
