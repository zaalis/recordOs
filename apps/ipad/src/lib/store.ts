/** App-wide state with a tiny external store (no framework dependency). */
import { type AppProfile, BUILTIN_PROFILES, type DeviceIdentity, cableSignalingUrls, ipadHotspotUrls } from '@vision-pro/protocol';
import { useSyncExternalStore } from 'react';
import { connection } from './connection.ts';
import { type AgentInfo, deviceKind, fetchAgentInfo, fetchNearby, type NearbyAgent } from './platform.ts';
import {
  DEFAULT_PREFERENCES,
  type KnownComputer,
  type Preferences,
  deleteProfile,
  forgetComputer,
  listComputers,
  listProfiles,
  loadIdentity,
  loadPreferences,
  saveComputer,
  saveIdentity,
  savePreferences,
  saveProfile,
} from './storage.ts';

export type Screen = 'loading' | 'insecure' | 'welcome' | 'home' | 'session';

export interface AppState {
  screen: Screen;
  ready: boolean;
  identity: DeviceIdentity | null;
  prefs: Preferences;
  computers: KnownComputer[];
  agent: AgentInfo | null;
  nearby: NearbyAgent[];
  customProfiles: AppProfile[];
}

let state: AppState = {
  screen: 'loading',
  ready: false,
  identity: null,
  prefs: DEFAULT_PREFERENCES,
  computers: [],
  agent: null,
  nearby: [],
  customProfiles: [],
};

const listeners = new Set<() => void>();

function set(patch: Partial<AppState>): void {
  state = { ...state, ...patch };
  for (const listener of listeners) listener();
}

export const store = {
  get: () => state,
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
};

export function useApp<T>(selector: (s: AppState) => T): T {
  return useSyncExternalStore(store.subscribe, () => selector(state));
}

export async function initialize(): Promise<void> {
  if (!window.isSecureContext || !globalThis.crypto?.subtle) {
    set({ screen: 'insecure', ready: true });
    return;
  }
  const [identity, prefs, computers, agent, customProfiles] = await Promise.all([
    loadIdentity(),
    loadPreferences(),
    listComputers(),
    fetchAgentInfo(),
    listProfiles(),
  ]);
  set({ identity, prefs, computers, agent, customProfiles, ready: true, screen: prefs.onboarded ? 'home' : 'welcome' });
  void refreshNearby();
}

export async function refreshNearby(): Promise<void> {
  const [agent, nearby] = await Promise.all([fetchAgentInfo(), fetchNearby()]);
  set({ agent, nearby });
  if (agent) void learnCable(agent.agentId, cableSignalingUrls(agent.usb?.urls));
}

/** Remembers the cable endpoints a paired Mac announces, for when Wi-Fi is not available. */
async function learnCable(agentId: string, urls: string[]): Promise<void> {
  const known = state.computers.find((c) => c.agentId === agentId);
  if (!known || !urls.length || JSON.stringify(known.usbUrls ?? []) === JSON.stringify(urls)) return;
  await rememberComputer({ ...known, usbUrls: urls });
}

/**
 * Cable endpoints to try for a Mac. When the app can ask the Mac (its origin answers), the Mac's
 * current answer is authoritative. Otherwise (no Wi-Fi), use the endpoints learned earlier and, on
 * an iPad, the addresses its Personal Hotspot gives a Mac over USB.
 */
export async function cableEndpoints(agentId: string, known: string[] = []): Promise<string[]> {
  const info = await fetchAgentInfo(AbortSignal.timeout(1500));
  if (info?.agentId === agentId) {
    const urls = cableSignalingUrls(info.usb?.urls);
    void learnCable(agentId, urls);
    return urls;
  }
  const computer = state.computers.find((c) => c.agentId === agentId);
  const guesses = deviceKind() === 'ipad' ? ipadHotspotUrls(computer?.port ?? 47800) : [];
  return cableSignalingUrls([...known, ...(computer?.usbUrls ?? []), ...guesses]);
}

connection.cablePaths = cableEndpoints;

export function setScreen(screen: Screen): void {
  set({ screen });
}

export function updatePrefs(patch: Partial<Preferences>): void {
  const prefs = { ...state.prefs, ...patch };
  set({ prefs });
  void savePreferences(prefs);
}

export async function rememberComputer(computer: KnownComputer): Promise<void> {
  await saveComputer(computer);
  set({ computers: await listComputers() });
}

export async function removeComputer(agentId: string): Promise<void> {
  await forgetComputer(agentId);
  set({ computers: await listComputers() });
}

export async function replaceIdentity(identity: DeviceIdentity): Promise<void> {
  await saveIdentity(identity);
  set({ identity });
}

export function allProfiles(): AppProfile[] {
  return [...BUILTIN_PROFILES, ...state.customProfiles];
}

export async function upsertProfile(profile: AppProfile): Promise<void> {
  await saveProfile(profile);
  set({ customProfiles: await listProfiles() });
}

export async function removeProfile(id: string): Promise<void> {
  await deleteProfile(id);
  set({ customProfiles: await listProfiles() });
  if (state.prefs.activeProfileId === id) updatePrefs({ activeProfileId: 'macos' });
}
