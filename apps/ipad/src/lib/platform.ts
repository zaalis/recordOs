/** Runtime environment: the agent serving this app, device traits and fullscreen. */
import type { DeviceKind } from '@vision-pro/protocol';

export interface AgentInfo {
  product: string;
  version: string;
  agentId: string;
  name: string;
  fingerprint: string;
  hostname: string;
  port: number;
  /** `code`: the PC is showing a pairing code right now. */
  pairing: { code: boolean };
  /** Cable endpoints of the PC right now (a tablet connected by USB). */
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

export function deviceKind(): DeviceKind {
  const ua = navigator.userAgent;
  if (/iPad/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1)) return 'ipad';
  if (navigator.maxTouchPoints > 1) return 'tablet';
  return 'browser';
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
