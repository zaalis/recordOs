/** Streaming settings, capabilities and statistics shared by the iPad app and the agent. */

export type CodecId = 'h264' | 'hevc' | 'av1';
export type CodecSetting = 'auto' | CodecId;
export type LatencyProfile = 'balanced' | 'low-latency' | 'max-quality' | 'battery-saver';
export type ResolutionMode = 'auto' | '720p' | '1080p' | '1440p' | '2160p' | '4320p' | 'native' | 'custom';
export type FpsSetting = 'auto' | 30 | 60 | 90 | 120;
export type BitrateMode = 'auto' | 'low' | 'medium' | 'high' | 'max' | 'custom';

export interface VideoSettings {
  profile: LatencyProfile;
  resolution: ResolutionMode;
  customWidth?: number;
  customHeight?: number;
  fps: FpsSetting;
  bitrate: BitrateMode;
  customKbps?: number;
  codec: CodecSetting;
}

export const DEFAULT_VIDEO_SETTINGS: VideoSettings = {
  // 1080p H.264 at 60 FPS: every iPad decodes it in hardware, and a 30 FPS
  // stream alone added up to ~50 ms of frame waiting and display time to
  // every interaction. Higher modes remain available as explicit choices.
  profile: 'balanced',
  resolution: '1080p',
  fps: 60,
  bitrate: 'auto',
  codec: 'h264',
};

export interface LatencyProfileSpec {
  id: LatencyProfile;
  name: string;
  summary: string;
  /** Upper bound for "auto" resolution, in vertical pixels. `null` = native. */
  maxAutoHeight: number | null;
  autoFps: 30 | 60 | 120;
  /** Bits per pixel per frame used to derive an automatic bitrate. */
  bitsPerPixel: number;
  minKbps: number;
  maxKbps: number;
  codecPreference: CodecId[];
}

export const LATENCY_PROFILES: Record<LatencyProfile, LatencyProfileSpec> = {
  balanced: {
    id: 'balanced',
    name: 'Balanced',
    summary: 'Sharp image, smooth motion. The right choice for most work.',
    maxAutoHeight: 1440,
    autoFps: 60,
    bitsPerPixel: 0.1,
    minKbps: 4_000,
    maxKbps: 40_000,
    codecPreference: ['h264', 'hevc', 'av1'],
  },
  'low-latency': {
    id: 'low-latency',
    name: 'Low Latency',
    summary: 'Fastest response for drawing, sculpting and precise input.',
    maxAutoHeight: 1080,
    autoFps: 120,
    bitsPerPixel: 0.08,
    minKbps: 4_000,
    maxKbps: 30_000,
    codecPreference: ['h264', 'hevc', 'av1'],
  },
  'max-quality': {
    id: 'max-quality',
    name: 'Maximum Quality',
    summary: 'Native resolution and a generous bitrate for color-critical work.',
    maxAutoHeight: null,
    autoFps: 60,
    bitsPerPixel: 0.16,
    minKbps: 10_000,
    maxKbps: 200_000,
    codecPreference: ['av1', 'hevc', 'h264'],
  },
  'battery-saver': {
    id: 'battery-saver',
    name: 'Battery Saver',
    summary: 'Lower resolution and frame rate to extend iPad battery life.',
    maxAutoHeight: 1080,
    autoFps: 30,
    bitsPerPixel: 0.07,
    minKbps: 2_000,
    maxKbps: 10_000,
    codecPreference: ['hevc', 'h264'],
  },
};

export const BITRATE_PRESETS_KBPS: Record<Exclude<BitrateMode, 'auto' | 'custom'>, number> = {
  low: 5_000,
  medium: 15_000,
  high: 30_000,
  max: 200_000,
};

export const RESOLUTION_HEIGHTS: Record<'720p' | '1080p' | '1440p' | '2160p' | '4320p', number> = {
  '720p': 720,
  '1080p': 1080,
  '1440p': 1440,
  '2160p': 2160,
  '4320p': 4320,
};

export interface ClientCodecSupport {
  codec: CodecId;
  /** Whether the iPad decodes this codec in hardware (MediaCapabilities `powerEfficient`). */
  powerEfficient: boolean;
}

export interface ClientDisplayInfo {
  /** Physical pixels of the iPad viewport. */
  width: number;
  height: number;
  /** Measured maximum refresh rate of the iPad display (60 or 120). */
  refreshHz: number;
}

export interface DisplayInfo {
  id: string;
  name: string;
  width: number;
  height: number;
  x: number;
  y: number;
  refreshHz: number;
  primary: boolean;
}

export interface EncoderInfo {
  codec: CodecId;
  hardware: boolean;
  name: string;
}

export interface AgentCaps {
  encoders: EncoderInfo[];
  displays: DisplayInfo[];
  activeDisplayId: string;
  pen: boolean;
  touch: boolean;
  maxFps: number;
}

export interface AppliedVideo {
  codec: CodecId;
  encoder: string;
  hardware: boolean;
  width: number;
  height: number;
  fps: number;
  targetKbps: number;
  /** Set when the requested codec could not be used. */
  fallbackReason?: string;
}

export interface AgentStats {
  captureFps: number;
  encodeFps: number;
  sentFps: number;
  captureMs: number;
  encodeMs: number;
  targetKbps: number;
  sentKbps: number;
  bweKbps: number;
  encoder: string;
  encoderState: 'running' | 'starting' | 'recovering' | 'idle';
  codec: CodecId;
  width: number;
  height: number;
  inputEventsPerSec: number;
  inputProcessMs: number;
  /** Average time a frame waited in the PC's send queue before leaving (absent on older PCs). */
  sendQueueMs?: number;
  /** Longest such wait during the last second. */
  sendQueueMaxMs?: number;
  /** Milliseconds per second the PC held encoding back because the network was congested. */
  heldMs?: number;
  /** Upper bound the PC asked of this iPad's jitter buffer, in ms (0: not accepted). */
  playoutDelayCapMs?: number;
}
