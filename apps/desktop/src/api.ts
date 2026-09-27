import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface SessionInfo {
  id: string; deviceId: string; deviceName: string; deviceKind: string; ip: string; startedAt: number;
  permission: 'control' | 'view'; streaming: boolean; video?: { codec: string; width: number; height: number; fps: number; targetKbps: number; hardware: boolean; encoder: string };
  stats?: { captureFps: number; sentFps: number; captureMs: number; encodeMs: number; sentKbps: number; inputProcessMs: number };
  rttMs?: number; activeApp?: string; viaRelay: boolean; link: 'lan' | 'usb' | 'relay'; localIp?: string;
}
export type TabletKind = 'ipad' | 'iphone' | 'apple' | 'android';
export interface UsbTablet { id: string; kind: TabletKind; name: string; vendorId: number; productId: number; driverMissing: boolean; }
export interface UsbLink {
  id: string; alias: string; description: string; interfaceIndex: number; localIp: string; peerIp?: string | null; kind: TabletKind; deviceName: string; url: string;
  network: 'public' | 'private' | 'domain' | 'unknown'; firewall: 'allowed' | 'blocked' | 'unknown'; internet: boolean; simulated: boolean;
}
export interface UsbStatus { enabled: boolean; devices: UsbTablet[]; links: UsbLink[]; }
export interface RelayStatus { state: 'disabled' | 'connecting' | 'connected' | 'error'; url?: string | null; error?: string | null; connectedAt?: number | null; udp: boolean; }
export interface PairingRequest { id: string; deviceId: string; deviceName: string; deviceKind: string; ip: string; sas: string; method: string; createdAt: number; expiresAt: number; }
export interface AgentStatus {
  running: boolean; error?: string; startedAt?: number; computerName: string; agentId: string; fingerprint: string; hostname: string; port: number; addresses: string[];
  networkMode: 'local-only' | 'remote-access'; relay: RelayStatus; sessions: SessionInfo[]; pendingPairing?: PairingRequest; connectWindowOpen: boolean;
  caps: { encoders: { codec: string; hardware: boolean; name: string }[]; displays: { id: string; name: string; width: number; height: number; refreshHz: number; primary: boolean }[]; activeDisplayId: string; pen: boolean; touch: boolean; maxFps: number };
  lockedSources: number; storeIntegrityWarning: boolean; version: string; usb: UsbStatus;
}
export interface Device { id: string; name: string; kind: string; pairedAt: number; lastSeenAt?: number; lastIp?: string; status: 'trusted' | 'untrusted' | 'revoked'; permission: 'control' | 'view'; pairingMethod: string; online: boolean; fingerprintDisplay: string; }
export interface Permissions { screenRecording: boolean; accessibility: boolean; }
export type PermissionKind = 'screen-recording' | 'accessibility' | 'local-network';
export interface ConnectInfo { url: string; qrSvg: string; code: string; expiresAt: number; computerName: string; fingerprint: string; setupUrl?: string; setupQrSvg?: string; authorityFingerprint: string; }
export interface AuditEntry { ts: number; severity: 'info' | 'notice' | 'warning' | 'critical'; kind: string; message: string; ip?: string; deviceName?: string; }
export interface Settings {
  schema: number; computerName: string;
  network: { mode: 'local-only' | 'remote-access'; httpsPort: number; advertiseMdns: boolean; allowNearbyPairing: boolean; relayUrl?: string | null; relayToken?: string | null; stunServers: string[]; usbDirect: boolean };
  security: { confirmEveryConnection: boolean; sessionMaxMinutes: number; trustExpiryDays: number; lockoutThreshold: number; lockoutMinutes: number; pairingTimeoutSeconds: number; notifyOnConnect: boolean; emergencyShortcut: string };
  streaming: { video: { profile: string; resolution: string; fps: 'auto' | number; bitrate: string; codec: string }; encoder: 'auto' | 'hardware-only' | 'software'; allowClientOverrides: boolean };
  input: { allowKeyboard: boolean; allowMouse: boolean; allowPen: boolean; allowTouch: boolean; penMode: 'tablet' | 'mouse'; detectActiveApp: boolean };
  display: { displayId?: string; preventSleep: boolean };
  advanced: { developerLoopback: boolean; verboseLogging: boolean; startServiceOnLaunch: boolean; launchAtLogin: boolean };
}

const isTauri = '__TAURI_INTERNALS__' in window;
const demoSettings: Settings = {
  schema: 2, computerName: "Bryan's MacBook Pro", network: { mode: 'local-only', httpsPort: 47800, advertiseMdns: true, allowNearbyPairing: true, stunServers: [], usbDirect: true },
  security: { confirmEveryConnection: false, sessionMaxMinutes: 720, trustExpiryDays: 90, lockoutThreshold: 5, lockoutMinutes: 15, pairingTimeoutSeconds: 120, notifyOnConnect: true, emergencyShortcut: 'Super+Alt+Shift+Backspace' },
  streaming: { video: { profile: 'max-quality', resolution: '2160p', fps: 'auto', bitrate: 'max', codec: 'auto' }, encoder: 'auto', allowClientOverrides: true },
  input: { allowKeyboard: true, allowMouse: true, allowPen: true, allowTouch: true, penMode: 'tablet', detectActiveApp: true }, display: { preventSleep: true },
  advanced: { developerLoopback: false, verboseLogging: false, startServiceOnLaunch: true, launchAtLogin: false },
};
let demoStatus: AgentStatus = { running: true, computerName: demoSettings.computerName, agentId: 'desktop-preview', fingerprint: '78ED 88B6 50AB 3F4C', hostname: 'vp-preview.local', port: 47800, addresses: ['https://vp-preview.local:47800'], networkMode: 'local-only', relay: { state: 'disabled', udp: false }, sessions: [], connectWindowOpen: false, caps: { encoders: [{ codec: 'h264', hardware: true, name: 'Apple M3 Pro H.264 (VideoToolbox)' }, { codec: 'hevc', hardware: true, name: 'Apple M3 Pro HEVC (VideoToolbox)' }, { codec: 'h264', hardware: false, name: 'OpenH264 (software)' }], displays: [{ id: '1', name: 'Built-in Liquid Retina XDR Display', width: 3024, height: 1964, refreshHz: 120, primary: true }], activeDisplayId: '1', pen: true, touch: false, maxFps: 120 }, lockedSources: 0, storeIntegrityWarning: false, version: '0.1.0',
  usb: { enabled: true, devices: [{ id: '05ac:12ab', kind: 'ipad', name: 'iPad Pro', vendorId: 0x05ac, productId: 0x12ab, driverMissing: false }], links: [{ id: 'en7', alias: 'en7', description: 'iPad USB', interfaceIndex: 18, localIp: '172.20.10.2', peerIp: '172.20.10.1', kind: 'ipad', deviceName: 'iPad Pro', url: 'wss://172.20.10.2:47800/ws', network: 'private', firewall: 'allowed', internet: false, simulated: false }] } };

export const api = {
  status: () => isTauri ? invoke<AgentStatus>('get_status') : Promise.resolve({ ...demoStatus }),
  settings: () => isTauri ? invoke<Settings>('get_settings') : Promise.resolve(structuredClone(demoSettings)),
  devices: () => isTauri ? invoke<Device[]>('get_devices') : Promise.resolve([]),
  audit: (limit = 100) => isTauri ? invoke<AuditEntry[]>('get_audit', { limit }) : Promise.resolve([]),
  service: async (running: boolean) => { if (isTauri) await invoke('set_service', { running }); else demoStatus = { ...demoStatus, running }; },
  openConnect: async (refresh = false) => isTauri ? invoke<ConnectInfo>('open_connect', { refresh }) : { url: 'https://vp-preview.local:47800/', qrSvg: '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="white"/><path d="M1 1h3v3H1zm5 0h3v3H6zM1 6h3v3H1zm5 0h1v1H6zm2 0h1v3H8zM6 8h1v1H6z" fill="black"/></svg>', code: '482 913', expiresAt: Date.now() + 120000, computerName: demoSettings.computerName, fingerprint: demoStatus.fingerprint, authorityFingerprint: 'A1:B2:C3:D4' },
  closeConnect: () => isTauri ? invoke('close_connect') : Promise.resolve(),
  respondPairing: (id: string, approve: boolean) => invoke<boolean>('respond_pairing', { id, approve }),
  deviceAction: (id: string, action: string, value?: string) => invoke<boolean>('device_action', { id, action, value }),
  saveSettings: async (settings: Settings) => { if (isTauri) await invoke('save_settings', { settings }); else { Object.assign(demoSettings, structuredClone(settings)); demoStatus = { ...demoStatus, networkMode: settings.network.mode }; } },
  emergency: () => isTauri ? invoke<number>('emergency_disconnect') : Promise.resolve(0),
  unlock: () => isTauri ? invoke('unlock_sources') : Promise.resolve(),
  permissions: () => isTauri ? invoke<Permissions>('get_permissions') : Promise.resolve({ screenRecording: true, accessibility: false }),
  requestPermission: (kind: PermissionKind) => isTauri ? invoke('request_permission', { kind }) : Promise.resolve(),
  openPermissionSettings: (kind: PermissionKind) => isTauri ? invoke('open_permission_settings', { kind }) : Promise.resolve(),
  allowUsbLink: async (id: string) => { if (isTauri) await invoke('allow_usb_link', { id }); else demoStatus = { ...demoStatus, usb: { ...demoStatus.usb, links: demoStatus.usb.links.map((l) => l.id === id ? { ...l, network: 'private', firewall: 'allowed', internet: false } : l) } }; },
  onEvent: (callback: () => void) => isTauri ? listen('agent-event', callback) : Promise.resolve(() => undefined),
};
