/** Runtime environment: the agent serving this app, device traits and media capabilities. */
import type { ClientCodecSupport, CodecId, DeviceKind } from '@vision-pro/protocol';

export interface AgentInfo {
  product: string;
  version: string;
  agentId: string;
  name: string;
  fingerprint: string;
  hostname: string;
  port: number;
  pairing: { nearby: boolean };
  /** Cable endpoints of the Mac right now (a tablet connected by USB). */
  usb?: { urls: string[] };
}

export interface NearbyAgent {
  agentId: string;
  name: string;
  hostname: string;
  port: number;
}

export async function fetchAgentInfo(signal?: AbortSignal): Promise<AgentInfo | null> {
  try {
    const response = await fetch('/api/info', { cache: 'no-store', signal });
    if (!response.ok) return null;
    const info = (await response.json()) as AgentInfo;
    if (info.product !== 'Vision-Pro' || typeof info.agentId !== 'string' || info.agentId.length !== 22) return null;
    return info;
  } catch {
    return null;
  }
}

export async function fetchNearby(): Promise<NearbyAgent[]> {
  try {
    const response = await fetch('/api/nearby', { cache: 'no-store' });
    if (!response.ok) return [];
    const list = (await response.json()) as NearbyAgent[];
    return Array.isArray(list) ? list.slice(0, 32) : [];
  } catch {
    return [];
  }
}

export function signalingUrl(): string {
  const scheme = location.protocol === 'https:' ? 'wss' : 'ws';
  return `${scheme}://${location.host}/ws`;
}

export function isSecure(): boolean {
  return window.isSecureContext && !!globalThis.crypto?.subtle;
}

export function deviceKind(): DeviceKind {
  const ua = navigator.userAgent;
  if (/iPad/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1)) return 'ipad';
  if (navigator.maxTouchPoints > 1) return 'tablet';
  return 'browser';
}

export function isStandalone(): boolean {
  return window.matchMedia('(display-mode: standalone), (display-mode: fullscreen)').matches || (navigator as { standalone?: boolean }).standalone === true;
}

const CODEC_MIME: Record<CodecId, string> = { h264: 'video/H264', hevc: 'video/H265', av1: 'video/AV1' };

export async function detectCodecs(): Promise<ClientCodecSupport[]> {
  const capabilities = typeof RTCRtpReceiver !== 'undefined' ? RTCRtpReceiver.getCapabilities?.('video') : null;
  const mimes = new Set((capabilities?.codecs ?? []).map((c) => c.mimeType.toLowerCase()));
  const results: ClientCodecSupport[] = [];
  for (const codec of Object.keys(CODEC_MIME) as CodecId[]) {
    const mime = CODEC_MIME[codec];
    if (!mimes.has(mime.toLowerCase())) continue;
    // Ask iPadOS about the demanding case we use by default. This does not
    // install a driver: Safari/iPadOS owns the decoder. It lets the Mac
    // prefer a genuinely hardware-decoded 4K codec and safely fall back.
    let powerEfficient = true;
    try {
      const query = navigator.mediaCapabilities?.decodingInfo({
        type: 'webrtc',
        video: { contentType: mime, width: 3840, height: 2160, bitrate: 200_000_000, framerate: 60 },
      } as MediaDecodingConfiguration);
      let timeout = 0;
      const info = query && await Promise.race([
        query,
        new Promise<undefined>((resolve) => { timeout = window.setTimeout(() => resolve(undefined), 500); }),
      ]).finally(() => window.clearTimeout(timeout));
      if (info) powerEfficient = info.supported && info.powerEfficient;
    } catch {
      // Some iPadOS versions do not expose a 4K WebRTC query. SDP negotiation
      // remains the authoritative compatibility check in that case.
    }
    results.push({ codec, powerEfficient });
  }
  return results;
}

/** Measures the display refresh rate from requestAnimationFrame timing (60 or 120 on ProMotion). */
export function measureRefreshRate(samples = 40): Promise<number> {
  return new Promise((resolve) => {
    if (document.hidden) { resolve(60); return; }
    const deltas: number[] = [];
    let last = 0;
    let frame = 0;
    let done = false;
    const finish = (hz: number) => {
      if (done) return;
      done = true;
      window.clearTimeout(timeout);
      cancelAnimationFrame(frame);
      resolve(hz);
    };
    // Safari can suspend animation frames while a tab is being restored.
    // A missing sample must never block the connection itself.
    const timeout = window.setTimeout(() => finish(60), 1_500);
    const step = (t: number) => {
      if (done) return;
      if (last) deltas.push(t - last);
      last = t;
      if (deltas.length < samples) frame = requestAnimationFrame(step);
      else {
        deltas.sort((a, b) => a - b);
        const median = deltas[Math.floor(deltas.length / 2)] ?? 16.7;
        const hz = 1000 / median;
        finish(hz > 100 ? 120 : hz > 80 ? 90 : 60);
      }
    };
    frame = requestAnimationFrame(step);
  });
}

export function physicalViewport(): { width: number; height: number } {
  const dpr = Math.min(window.devicePixelRatio || 1, 3);
  return {
    width: Math.round(Math.max(100, window.innerWidth * dpr)),
    height: Math.round(Math.max(100, window.innerHeight * dpr)),
  };
}

export async function toggleFullscreen(): Promise<boolean> {
  const doc = document as Document & { webkitFullscreenElement?: Element; webkitExitFullscreen?: () => Promise<void> };
  const root = document.documentElement as HTMLElement & { webkitRequestFullscreen?: () => Promise<void> };
  try {
    if (doc.fullscreenElement || doc.webkitFullscreenElement) {
      const exit = doc.exitFullscreen ?? doc.webkitExitFullscreen;
      if (!exit) return false;
      await exit.call(doc);
      return false;
    }
    if (root.requestFullscreen) await root.requestFullscreen({ navigationUI: 'hide' });
    else if (root.webkitRequestFullscreen) await root.webkitRequestFullscreen();
    else return false;
    return true;
  } catch {
    return false;
  }
}

export function isFullscreenActive(): boolean {
  const doc = document as Document & { webkitFullscreenElement?: Element };
  return Boolean(doc.fullscreenElement || doc.webkitFullscreenElement);
}

export function fullscreenSupported(): boolean {
  const root = document.documentElement as HTMLElement & { webkitRequestFullscreen?: unknown };
  return typeof root.requestFullscreen === 'function' || typeof root.webkitRequestFullscreen === 'function';
}
