/**
 * Local persistence (IndexedDB). The device private key is a non-extractable CryptoKey: script can
 * use it to sign, but its bytes can never be read or exported — not even by this app.
 */
import { type AppProfile, type DeviceIdentity, generateDeviceIdentity } from '@vision-pro/protocol';

const DB_NAME = 'vision-pro';
const DB_VERSION = 1;

export interface KnownComputer {
  agentId: string;
  name: string;
  fingerprint: string;
  hostname: string;
  port: number;
  pairedAt: number;
  lastConnectedAt: number | null;
  /** Relay announced by this PC during its last session; used when the PC is not on this network. */
  relayUrl?: string;
  /** USB cable endpoints this PC announced; tried when the PC is not reachable over Wi-Fi. */
  usbUrls?: string[];
}

export type ControlMode = 'direct' | 'trackpad' | 'pencil';
export type ThreeFingerAction = 'none' | 'task-view' | 'show-desktop' | 'switch-app' | 'desktops';
export type PencilFingers = 'navigate' | 'ignore' | 'touch';

export interface SurfaceStyle {
  background: 'graphite' | 'midnight' | 'paper' | 'custom';
  backgroundColor: string;
  pattern: 'dots' | 'grid' | 'none';
  patternColor: string;
  patternOpacity: number;
  patternSpacing: number;
  trail: boolean;
  trailColor: string;
  trailSize: number;
  trailDuration: number;
}
export const DEFAULT_SURFACE_STYLE: SurfaceStyle = {
  background: 'graphite', backgroundColor: '#151518', pattern: 'dots', patternColor: '#dcdce0', patternOpacity: 0.19, patternSpacing: 13,
  trail: false, trailColor: '#8ebaff', trailSize: 14, trailDuration: 600,
};
export interface Preferences {
  surfaceStyle: SurfaceStyle;
  deviceName: string;
  onboarded: boolean;
  controlMode: ControlMode;
  trackingSpeed: number;
  scrollSpeed: number;
  naturalScrolling: boolean;
  tapToClick: boolean;
  threeFingerSwipe: ThreeFingerAction;
  pencilWindowsInk: boolean;
  pencilHover: boolean;
  pencilPressureCurve: 'soft' | 'linear' | 'firm';
  pencilFingers: PencilFingers;
  twoFingerTapUndo: boolean;
  commandAsCtrl: boolean;
  autoHideToolbar: boolean;
  autoProfiles: boolean;
  showStats: boolean;
  toolbar: { edge: 'bottom' | 'top' | 'left' | 'right'; offset: number; collapsed: boolean };
  activeProfileId: string;
  keyboardExtras: string[];
}

export const DEFAULT_PREFERENCES: Preferences = {
  surfaceStyle: { ...DEFAULT_SURFACE_STYLE },
  deviceName: 'iPad',
  onboarded: false,
  controlMode: 'direct',
  trackingSpeed: 1,
  scrollSpeed: 1,
  naturalScrolling: true,
  tapToClick: true,
  threeFingerSwipe: 'task-view',
  pencilWindowsInk: true,
  pencilHover: true,
  pencilPressureCurve: 'linear',
  pencilFingers: 'navigate',
  twoFingerTapUndo: true,
  commandAsCtrl: true,
  autoHideToolbar: true,
  autoProfiles: true,
  showStats: false,
  toolbar: { edge: 'bottom', offset: 0.5, collapsed: false },
  activeProfileId: 'windows',
  keyboardExtras: [],
};

let dbPromise: Promise<IDBDatabase> | null = null;

function db(): Promise<IDBDatabase> {
  dbPromise ??= new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains('kv')) database.createObjectStore('kv');
      if (!database.objectStoreNames.contains('computers')) database.createObjectStore('computers', { keyPath: 'agentId' });
      if (!database.objectStoreNames.contains('profiles')) database.createObjectStore('profiles', { keyPath: 'id' });
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  }).catch((error: unknown) => {
    // A transient IndexedDB failure must not poison all later retries.
    dbPromise = null;
    throw error;
  });
  return dbPromise as Promise<IDBDatabase>;
}

function run<T>(store: string, mode: IDBTransactionMode, fn: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  return db().then(
    (database) =>
      new Promise<T>((resolve, reject) => {
        const tx = database.transaction(store, mode);
        const request = fn(tx.objectStore(store));
        let result: T;
        request.onsuccess = () => { result = request.result; };
        tx.oncomplete = () => resolve(result);
        tx.onabort = () => reject(tx.error ?? request.error ?? new Error('IndexedDB transaction aborted'));
      }),
  );
}

export async function loadIdentity(): Promise<DeviceIdentity> {
  const existing = await run<DeviceIdentity | undefined>('kv', 'readonly', (s) => s.get('identity'));
  if (existing?.privateKey && existing.publicRaw) {
    return { ...existing, publicRaw: new Uint8Array(existing.publicRaw) };
  }
  const identity = await generateDeviceIdentity();
  await saveIdentity(identity);
  return identity;
}

export async function saveIdentity(identity: DeviceIdentity): Promise<void> {
  await run('kv', 'readwrite', (s) => s.put(identity, 'identity'));
}

export async function loadPreferences(): Promise<Preferences> {
  const stored = await run<Partial<Preferences> | undefined>('kv', 'readonly', (s) => s.get('preferences'));
  return preferencesFrom(stored);
}

/** Complete preferences from what an earlier version stored. */
export function preferencesFrom(stored: Partial<Preferences> | undefined): Preferences {
  // Drop what earlier versions stored and no longer exists: the language (Record OS is
  // French-only), the screen-streaming options (the iPad is a tablet surface only).
  const {
    language: _language,
    autoHideToolbar: _autoHide,
    stageMode: _stageMode,
    video: _video,
    videoDefaultVersion: _videoVersion,
    pinchAction: _pinch,
    showLocalCursor: _cursor,
    ...saved
  } = (stored ?? {}) as Partial<Preferences> & Record<'language' | 'stageMode' | 'video' | 'videoDefaultVersion' | 'pinchAction' | 'showLocalCursor', unknown>;
  return {
    ...DEFAULT_PREFERENCES,
    ...saved,
    surfaceStyle: normalizeSurfaceStyle(saved.surfaceStyle),
    toolbar: { ...DEFAULT_PREFERENCES.toolbar, ...saved.toolbar },
    // Session controls always disappear after five seconds. This migration
    // also fixes iPads where an older build persisted the option as disabled.
    autoHideToolbar: true,
  };
}

export async function savePreferences(prefs: Preferences): Promise<void> {
  await run('kv', 'readwrite', (s) => s.put(prefs, 'preferences'));
}

export async function listComputers(): Promise<KnownComputer[]> {
  const list = await run<KnownComputer[]>('computers', 'readonly', (s) => s.getAll());
  return list.sort((a, b) => (b.lastConnectedAt ?? 0) - (a.lastConnectedAt ?? 0));
}

export async function saveComputer(computer: KnownComputer): Promise<void> {
  await run('computers', 'readwrite', (s) => s.put(computer));
}

export async function forgetComputer(agentId: string): Promise<void> {
  await run('computers', 'readwrite', (s) => s.delete(agentId));
}

export async function listProfiles(): Promise<AppProfile[]> {
  return run<AppProfile[]>('profiles', 'readonly', (s) => s.getAll());
}

export async function saveProfile(profile: AppProfile): Promise<void> {
  await run('profiles', 'readwrite', (s) => s.put(profile));
}

export async function deleteProfile(id: string): Promise<void> {
  await run('profiles', 'readwrite', (s) => s.delete(id));
}

/** Removes every local secret and pairing (Settings → Reset this iPad). */
export async function resetEverything(onBlocked?: () => void): Promise<void> {
  const database = await db();
  database.close();
  dbPromise = null;
  await new Promise<void>((resolve, reject) => {
    const request = indexedDB.deleteDatabase(DB_NAME);
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error);
    request.onblocked = () => onBlocked?.();
  });
}

export function normalizeSurfaceStyle(value?: Partial<SurfaceStyle>): SurfaceStyle {
  const v = { ...DEFAULT_SURFACE_STYLE, ...value };
  const color = (s: string, fallback: string) => /^#[0-9a-f]{6}$/i.test(s) ? s : fallback;
  const range = (n: number, min: number, max: number, fallback: number) => Number.isFinite(n) ? Math.max(min, Math.min(max, n)) : fallback;
  return { ...v,
    background: ['graphite','midnight','paper','custom'].includes(v.background) ? v.background : 'graphite',
    pattern: ['dots','grid','none'].includes(v.pattern) ? v.pattern : 'dots',
    backgroundColor: color(v.backgroundColor, DEFAULT_SURFACE_STYLE.backgroundColor),
    patternColor: color(v.patternColor, DEFAULT_SURFACE_STYLE.patternColor),
    trailColor: color(v.trailColor, DEFAULT_SURFACE_STYLE.trailColor),
    patternOpacity: range(v.patternOpacity, 0, 1, 0.19), patternSpacing: range(v.patternSpacing, 8, 48, 13),
    trailSize: range(v.trailSize, 2, 40, 14), trailDuration: range(v.trailDuration, 100, 1500, 600), trail: v.trail === true,
  };
}
