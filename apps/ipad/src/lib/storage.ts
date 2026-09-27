/**
 * Local persistence (IndexedDB). The device private key is a non-extractable CryptoKey: script can
 * use it to sign, but its bytes can never be read or exported — not even by this app.
 */
import { type AppProfile, type DeviceIdentity, generateDeviceIdentity, type VideoSettings, DEFAULT_VIDEO_SETTINGS } from '@vision-pro/protocol';

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
}

export type ControlMode = 'direct' | 'trackpad' | 'pencil';
export type StageMode = 'stream' | 'tablet';
export type PinchAction = 'zoom-view' | 'zoom-app';
export type ThreeFingerAction = 'none' | 'mission-control' | 'show-desktop' | 'switch-app' | 'spaces';
export type PencilFingers = 'navigate' | 'ignore' | 'touch';

export interface Preferences {
  deviceName: string;
  onboarded: boolean;
  controlMode: ControlMode;
  stageMode: StageMode;
  video: VideoSettings;
  /** Lets us safely migrate only former product-default streaming presets. */
  videoDefaultVersion: number;
  trackingSpeed: number;
  scrollSpeed: number;
  naturalScrolling: boolean;
  tapToClick: boolean;
  pinchAction: PinchAction;
  threeFingerSwipe: ThreeFingerAction;
  /** Apple Pencil acts as a macOS tablet pen (pressure, tilt) instead of a plain mouse. */
  pencilTablet: boolean;
  pencilHover: boolean;
  pencilPressureCurve: 'soft' | 'linear' | 'firm';
  pencilFingers: PencilFingers;
  twoFingerTapUndo: boolean;
  autoHideToolbar: boolean;
  autoProfiles: boolean;
  showStats: boolean;
  showLocalCursor: boolean;
  toolbar: { edge: 'bottom' | 'top' | 'left' | 'right'; offset: number; collapsed: boolean };
  activeProfileId: string;
  keyboardExtras: string[];
}

export const DEFAULT_PREFERENCES: Preferences = {
  deviceName: 'iPad',
  onboarded: false,
  controlMode: 'direct',
  stageMode: 'stream',
  video: DEFAULT_VIDEO_SETTINGS,
  videoDefaultVersion: 3,
  trackingSpeed: 1,
  scrollSpeed: 1,
  naturalScrolling: true,
  tapToClick: true,
  // Pinch controls the remote app by default. The former local-only zoom is
  // still available in Settings for users who need to inspect small UI text.
  pinchAction: 'zoom-app',
  threeFingerSwipe: 'mission-control',
  pencilTablet: true,
  pencilHover: true,
  pencilPressureCurve: 'linear',
  pencilFingers: 'navigate',
  twoFingerTapUndo: true,
  autoHideToolbar: true,
  autoProfiles: true,
  showStats: false,
  showLocalCursor: true,
  toolbar: { edge: 'bottom', offset: 0.5, collapsed: false },
  activeProfileId: 'macos',
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
  // Drop the language preference written by earlier versions: Vision-Pro is
  // intentionally French-only now.
  const { language: _legacyLanguage, autoHideToolbar: _legacyAutoHide, ...saved } = (stored ?? {}) as Partial<Preferences> & { language?: unknown };
  const video = { ...DEFAULT_PREFERENCES.video, ...saved.video };
  // Migrate only exact former defaults. Any combination the user deliberately
  // changed in the settings panel remains untouched.
  const legacyAggressiveVideo = saved.videoDefaultVersion !== 3
    && video.profile === 'max-quality'
    && video.resolution === '2160p'
    && video.fps === 'auto'
    && video.bitrate === 'max'
    && video.codec === 'auto';
  const legacyResponsiveVideo = saved.videoDefaultVersion !== 3
    && video.profile === 'balanced'
    && video.resolution === 'auto'
    && video.fps === 'auto'
    && video.bitrate === 'auto'
    && video.codec === 'auto';
  return {
    ...DEFAULT_PREFERENCES,
    ...saved,
    video: legacyAggressiveVideo || legacyResponsiveVideo ? { ...DEFAULT_PREFERENCES.video } : video,
    videoDefaultVersion: 3,
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
