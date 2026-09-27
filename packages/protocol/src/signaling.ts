/**
 * Signaling messages. Handshake messages are plaintext (inside TLS); everything after the
 * handshake travels inside `SecureChannel` frames, end-to-end encrypted between iPad and Mac,
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
  | 'internal';

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

export type SessionEndReason = 'user' | 'revoked' | 'expired' | 'shutdown' | 'replaced' | 'error' | 'stopped';

export interface SessionEnd {
  t: 'session.end';
  reason: SessionEndReason;
}

export type ServerSecureMessage = SessionReady | PairPending | PairResult | RtcAnswer | SessionEnd;

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
