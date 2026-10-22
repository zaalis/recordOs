import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface SessionInfo {
  id: string; deviceId: string; deviceName: string; deviceKind: string; ip: string; startedAt: number;
  permission: 'control' | 'view';
  /** Input events received per second. */
  inputRate: number;
  rttMs?: number; activeApp?: string; viaRelay: boolean; link: 'lan' | 'usb' | 'relay'; localIp?: string;
}
export type TabletKind = 'ipad' | 'iphone' | 'apple' | 'android';
export interface UsbTablet { id: string; kind: TabletKind; name: string; vendorId: number; productId: number; driverMissing: boolean; }
export interface UsbLink {
  id: string; alias: string; description: string; interfaceIndex: number; localIp: string; peerIp?: string | null; kind: TabletKind; deviceName: string; url: string;
  network: 'public' | 'private' | 'domain' | 'unknown'; firewall: 'allowed' | 'blocked' | 'unknown'; internet: boolean; simulated: boolean;
}
export interface UsbStatus { enabled: boolean; devices: UsbTablet[]; links: UsbLink[]; }
export interface RelayStatus { state: 'disabled' | 'connecting' | 'connected' | 'error'; url?: string | null; error?: string | null; connectedAt?: number | null; }
export interface PairingRequest { id: string; deviceId: string; deviceName: string; deviceKind: string; ip: string; sas: string; method: string; createdAt: number; expiresAt: number; }
export interface AgentStatus {
  running: boolean; error?: string; startedAt?: number; computerName: string; agentId: string; fingerprint: string; fullFingerprint?: string; hostname: string; port: number; addresses: string[];
  networkMode: 'local-only' | 'remote-access'; relay: RelayStatus; sessions: SessionInfo[]; pendingPairing?: PairingRequest; connectWindowOpen: boolean;
  caps: { displays: { id: string; name: string; width: number; height: number; refreshHz: number; primary: boolean }[]; activeDisplayId: string; pen: boolean; touch: boolean };
  lockedSources: number; storeIntegrityWarning: boolean; version: string; usb: UsbStatus;
}
export interface Device { id: string; name: string; kind: string; pairedAt: number; lastSeenAt?: number; lastIp?: string; status: 'trusted' | 'untrusted' | 'revoked'; permission: 'control' | 'view'; pairingMethod: string; online: boolean; fingerprintDisplay: string; }
/** An address to type on the iPad: the setup page (trust profile once), which then opens the app. */
export interface ConnectAddress { network: 'lan' | 'tailscale'; url: string; appUrl: string; }
export interface ConnectInfo { code: string; expiresAt: number; computerName: string; fingerprint: string; addresses: ConnectAddress[]; authorityFingerprint: string; }
export interface AuditEntry { ts: number; severity: 'info' | 'notice' | 'warning' | 'critical'; kind: string; message: string; ip?: string; deviceName?: string; }
export interface Settings {
  schema: number; computerName: string;
  network: { mode: 'local-only' | 'remote-access'; httpsPort: number; advertiseMdns: boolean; relayUrl?: string | null; relayToken?: string | null; usbDirect: boolean };
  security: { confirmEveryConnection: boolean; sessionMaxMinutes: number; trustExpiryDays: number; lockoutThreshold: number; lockoutMinutes: number; pairingTimeoutSeconds: number; notifyOnConnect: boolean; emergencyShortcut: string };
  input: { allowKeyboard: boolean; allowMouse: boolean; allowPen: boolean; allowTouch: boolean; penMode: 'windows-ink' | 'mouse'; detectActiveApp: boolean };
  display: { displayId?: string; preventSleep: boolean };
  advanced: { developerLoopback: boolean; verboseLogging: boolean; startServiceOnLaunch: boolean; launchAtLogin: boolean };
}

const isTauri = '__TAURI_INTERNALS__' in window;
const demoSettings: Settings = {
  schema: 4, computerName: "Bryan's PC", network: { mode: 'local-only', httpsPort: 47800, advertiseMdns: true, usbDirect: true },
  security: { confirmEveryConnection: false, sessionMaxMinutes: 720, trustExpiryDays: 90, lockoutThreshold: 5, lockoutMinutes: 15, pairingTimeoutSeconds: 120, notifyOnConnect: true, emergencyShortcut: 'Ctrl+Alt+Shift+Backspace' },
  input: { allowKeyboard: true, allowMouse: true, allowPen: true, allowTouch: true, penMode: 'windows-ink', detectActiveApp: true }, display: { preventSleep: true },
  advanced: { developerLoopback: false, verboseLogging: false, startServiceOnLaunch: true, launchAtLogin: false },
};
let demoStatus: AgentStatus = { running: true, computerName: demoSettings.computerName, agentId: 'desktop-preview', fingerprint: '78ED 88B6 50AB 3F4C', hostname: 'vp-preview.local', port: 47800, addresses: ['https://vp-preview.local:47800', 'https://192.168.1.20:47800', 'https://100.101.102.103:47800'], networkMode: 'local-only', relay: { state: 'disabled' }, sessions: [], connectWindowOpen: false, caps: { displays: [{ id: 'display-1', name: 'Main Display', width: 2560, height: 1440, refreshHz: 144, primary: true }], activeDisplayId: 'display-1', pen: true, touch: true }, lockedSources: 0, storeIntegrityWarning: false, version: '0.1.0',
  usb: { enabled: true, devices: [{ id: '05ac:12ab', kind: 'ipad', name: 'iPad Pro', vendorId: 0x05ac, productId: 0x12ab, driverMissing: false }], links: [{ id: '{DEMO}', alias: 'Ethernet 3', description: 'Apple Mobile Device Ethernet', interfaceIndex: 12, localIp: '172.20.10.2', peerIp: '172.20.10.1', kind: 'ipad', deviceName: 'iPad Pro', url: 'wss://172.20.10.2:47800/ws', network: 'public', firewall: 'blocked', internet: true, simulated: false }] } };

export const api = {
  status: () => isTauri ? invoke<AgentStatus>('get_status') : Promise.resolve({ ...demoStatus }),
  settings: () => isTauri ? invoke<Settings>('get_settings') : Promise.resolve(structuredClone(demoSettings)),
  devices: () => isTauri ? invoke<Device[]>('get_devices') : Promise.resolve([]),
  audit: (limit = 100) => isTauri ? invoke<AuditEntry[]>('get_audit', { limit }) : Promise.resolve([]),
  service: async (running: boolean) => { if (isTauri) await invoke('set_service', { running }); else demoStatus = { ...demoStatus, running }; },
  openConnect: async (refresh = false) => isTauri ? invoke<ConnectInfo>('open_connect', { refresh }) : { code: '482913', expiresAt: Date.now() + 300000, computerName: demoSettings.computerName, fingerprint: demoStatus.fingerprint, addresses: [{ network: 'lan', url: 'http://192.168.1.20:47801', appUrl: 'https://192.168.1.20:47800/' }, { network: 'tailscale', url: 'http://100.101.102.103:47801', appUrl: 'https://100.101.102.103:47800/' }], authorityFingerprint: 'A1:B2:C3:D4' } satisfies ConnectInfo,
  closeConnect: () => isTauri ? invoke('close_connect') : Promise.resolve(),
  respondPairing: (id: string, approve: boolean) => invoke<boolean>('respond_pairing', { id, approve }),
  deviceAction: (id: string, action: string, value?: string) => invoke<boolean>('device_action', { id, action, value }),
  saveSettings: async (settings: Settings) => { if (isTauri) await invoke('save_settings', { settings }); else { Object.assign(demoSettings, structuredClone(settings)); demoStatus = { ...demoStatus, networkMode: settings.network.mode }; } },
  emergency: () => isTauri ? invoke<number>('emergency_disconnect') : Promise.resolve(0),
  unlock: () => isTauri ? invoke('unlock_sources') : Promise.resolve(),
  allowUsbLink: async (id: string) => { if (isTauri) await invoke('allow_usb_link', { id }); else demoStatus = { ...demoStatus, usb: { ...demoStatus.usb, links: demoStatus.usb.links.map((l) => l.id === id ? { ...l, network: 'private', firewall: 'allowed', internet: false } : l) } }; },
  onEvent: (callback: () => void) => isTauri ? listen('agent-event', callback) : Promise.resolve(() => undefined),
};
