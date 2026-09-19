/**
 * Signaling messages. Handshake messages are plaintext (inside TLS); everything after the
 * handshake travels inside `SecureChannel` frames, end-to-end encrypted between iPad and PC,
 * even when relayed by the optional account server.
 */
import { LIMITS, PROTOCOL_VERSION } from './constants.ts';
import type { AgentCaps, AppliedVideo, ClientCodecSupport, ClientDisplayInfo, DisplayInfo, VideoSettings } from './media.ts';
import { type Infer, v } from './validate.ts';

export type HandshakeErrorCode =
  | 'version'
  | 'rate_limited'
  | 'locked_out'
  | 'unknown_device'
  | 'revoked'
  | 'bad_signature'
  | 'pairing_disabled'
  | 'pairing_busy'
  | 'denied'
  | 'expired'
  | 'not_local'
  | 'protocol'
  | 'internal'
  /** Sent by a relay when the PC is not connected to it. */
  | 'offline';

export const deviceKinds = ['ipad', 'tablet', 'browser'] as const;
export type DeviceKind = (typeof deviceKinds)[number];

// --- Plaintext handshake ----------------------------------------------------

export interface ServerHello {
  t: 'hello';
  v: typeof PROTOCOL_VERSION;
  agentId: string;
  agentName: string;
  agentPub: string;
  commit: string;
  pairing: { nearby: boolean; code: boolean; qr: boolean };
}

export const clientInitSchema = v.object({
  t: v.literal('init'),
  v: v.literal(PROTOCOL_VERSION),
  mode: v.oneOf(['pair', 'auth'] as const),
  clientEph: v.b64u(65),
  nC: v.b64u(32),
  deviceId: v.string({ min: 22, max: 22, pattern: /^[A-Za-z0-9_-]+$/ }),
  devicePub: v.b64u(65),
  deviceName: v.string({ min: 1, max: LIMITS.maxDeviceName }),
  deviceKind: v.oneOf(deviceKinds),
});
export type ClientInit = Infer<typeof clientInitSchema>;

export interface ServerReveal {
  t: 'reveal';
  serverEph: string;
  nS: string;
  sig: string;
}

export const clientProveSchema = v.object({ t: v.literal('prove'), sig: v.b64u(64) });
export type ClientProve = Infer<typeof clientProveSchema>;

export interface HandshakeErrorMessage {
  t: 'error';
  code: HandshakeErrorCode;
  retryAfterMs?: number;
}

// --- Encrypted: server → client ---------------------------------------------

export type DevicePermission = 'control' | 'view';

export interface SessionReady {
  t: 'session.ready';
  sessionId: string;
  device: { id: string; name: string; permission: DevicePermission };
  agent: { id: string; name: string; version: string; hostname: string };
  caps: AgentCaps;
  expiresAt: number;
  /** The PC is also reachable through this Vision-Pro relay (learned inside the encrypted channel). */
  relay?: { url: string };
  /** Physical path of this session: Wi-Fi/Ethernet, a USB cable, or the Internet relay. */
  link?: SessionLink;
  /** Cable endpoints of this PC right now (a tablet connected by USB can move its session there). */
  usb?: { urls: string[] };
  /** Relayed sessions only: media must use these TURN servers. */
  ice?: RelayIceConfig;
}

export type SessionLink = 'lan' | 'usb' | 'relay';

/** Sent to local-network sessions when a USB cable path to the PC appears. */
export interface LinkUsb {
  t: 'link.usb';
  urls: string[];
}

function isPrivateV4(host: string): boolean {
  const parts = host.split('.').map(Number);
  if (parts.length !== 4 || parts.some((p) => !Number.isInteger(p) || p < 0 || p > 255)) return false;
  const [a, b] = parts as [number, number, number, number];
  return a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168);
}

/**
 * A cable signaling endpoint announced by the PC: `wss://<private IPv4>:<port>/ws`, or plain `ws://`
 * on a loopback address for development tests. Anything else is ignored.
 */
export function cableSignalingUrl(value: unknown): string | null {
  if (typeof value !== 'string' || value.length > 64) return null;
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return null;
  }
  if (url.pathname !== '/ws' || url.search || url.hash || url.username || url.password || !url.port) return null;
  if (url.protocol === 'wss:' && isPrivateV4(url.hostname)) return `wss://${url.host}/ws`;
  if (url.protocol === 'ws:' && /^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(url.hostname)) return `ws://${url.host}/ws`;
  return null;
}

/** Keeps valid, distinct cable endpoints (at most eight). */
export function cableSignalingUrls(values: unknown): string[] {
  if (!Array.isArray(values)) return [];
  const urls = values.map(cableSignalingUrl).filter((u): u is string => u !== null);
  return [...new Set(urls)].slice(0, 8);
}

/**
 * Addresses a PC receives from an iPad's Personal Hotspot over USB (172.20.10.0/28, the iPad
 * being .1). Used only when the PC cannot be reached otherwise and no cable address is known yet.
 */
export function ipadHotspotUrls(port: number): string[] {
  return [2, 3, 4, 5].map((host) => `wss://172.20.10.${host}:${port}/ws`);
}

export interface RelayIceConfig {
  iceServers: { urls: string[]; username: string; credential: string }[];
  iceTransportPolicy: 'relay';
}

/** Signaling endpoint of a PC on a relay. Only wss:// (or ws:// on loopback, for tests). */
export function relayConnectUrl(relayUrl: string, agentId: string): string | null {
  if (!/^[A-Za-z0-9_-]{22}$/.test(agentId)) return null;
  let url: URL;
  try {
    url = new URL(relayUrl);
  } catch {
    return null;
  }
  const loopback = url.hostname === 'localhost' || url.hostname === '127.0.0.1' || url.hostname === '[::1]';
  if (url.protocol !== 'wss:' && !(url.protocol === 'ws:' && loopback)) return null;
  if (url.username || url.password || url.search || url.hash) return null;
  return `${url.protocol}//${url.host}/v1/connect/${agentId}`;
}

export interface PairPending {
  t: 'pair.pending';
  expiresInMs: number;
}

export interface PairResult {
  t: 'pair.result';
  approved: boolean;
}

export interface RtcAnswer {
  t: 'rtc.answer';
  sdp: string;
  video: AppliedVideo;
}

export type SessionEndReason = 'user' | 'revoked' | 'expired' | 'shutdown' | 'replaced' | 'error' | 'stopped' | 'link';

export interface SessionEnd {
  t: 'session.end';
  reason: SessionEndReason;
}

export type ServerSecureMessage = SessionReady | PairPending | PairResult | RtcAnswer | SessionEnd | LinkUsb;

// --- Encrypted: client → server ---------------------------------------------

export const pairRequestSchema = v.object({
  t: v.literal('pair.request'),
  proof: v.optional(
    v.object({
      kind: v.oneOf(['qr', 'code'] as const),
      mac: v.b64u(32),
    }),
  ),
});
export type PairRequest = Infer<typeof pairRequestSchema>;

const videoSettingsSchema = v.object({
  profile: v.oneOf(['balanced', 'low-latency', 'max-quality', 'battery-saver'] as const),
  resolution: v.oneOf(['auto', '720p', '1080p', '1440p', '2160p', '4320p', 'native', 'custom'] as const),
  customWidth: v.optional(v.number({ min: 320, max: 7680, integer: true })),
  customHeight: v.optional(v.number({ min: 240, max: 4320, integer: true })),
  fps: v.union<VideoSettings['fps']>(v.literal('auto'), v.oneOf([30, 60, 90, 120] as const)),
  bitrate: v.oneOf(['auto', 'low', 'medium', 'high', 'max', 'custom'] as const),
  customKbps: v.optional(v.number({ min: 500, max: 200_000, integer: true })),
  codec: v.oneOf(['auto', 'h264', 'hevc', 'av1'] as const),
});
export { videoSettingsSchema };

export const rtcOfferSchema = v.object({
  t: v.literal('rtc.offer'),
  sdp: v.string({ min: 10, max: 48_000 }),
  video: videoSettingsSchema,
  display: v.object({
    width: v.number({ min: 100, max: 8192, integer: true }),
    height: v.number({ min: 100, max: 8192, integer: true }),
    refreshHz: v.number({ min: 24, max: 240 }),
  }),
  codecs: v.array(
    v.object({
      codec: v.oneOf(['h264', 'hevc', 'av1'] as const),
      powerEfficient: v.boolean(),
    }),
    8,
  ),
});
export type RtcOffer = Infer<typeof rtcOfferSchema> & {
  display: ClientDisplayInfo;
  codecs: ClientCodecSupport[];
};

export const sessionCloseSchema = v.object({ t: v.literal('session.close') });

export const deviceRotateSchema = v.object({
  t: v.literal('device.rotate'),
  newPub: v.b64u(65),
  sig: v.b64u(64),
});

export type ClientSecureMessage =
  | PairRequest
  | RtcOffer
  | Infer<typeof sessionCloseSchema>
  | Infer<typeof deviceRotateSchema>;

// --- Control data channel (JSON, DTLS-protected) ------------------------------

export type ControlToAgent =
  | { t: 'ping'; id: number; t0: number }
  | { t: 'video.settings'; video: VideoSettings }
  | { t: 'display.select'; displayId: string }
  | { t: 'keyframe' }
  | { t: 'app.watch'; enabled: boolean };

export type AppProfileId = 'blender' | 'maya' | 'photoshop';

export type ControlToClient =
  | { t: 'pong'; id: number; t0: number; agentMs: number }
  | { t: 'stats'; stats: import('./media.ts').AgentStats }
  | { t: 'cursor.shape'; id: number; width: number; height: number; hotX: number; hotY: number; png: string }
  | { t: 'app.active'; exe: string; profile: AppProfileId | null }
  | { t: 'displays'; displays: DisplayInfo[]; activeId: string }
  | { t: 'video.applied'; video: AppliedVideo }
  | { t: 'video.renegotiate'; reason: string }
  | { t: 'permission'; control: boolean };
