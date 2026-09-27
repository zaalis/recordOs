/** App-wide state with a tiny external store (no framework dependency). */
import { type AppProfile, BUILTIN_PROFILES, type DeviceIdentity } from '@vision-pro/protocol';
import { useSyncExternalStore } from 'react';
import { type AgentInfo, fetchAgentInfo, fetchNearby, type NearbyAgent } from './platform.ts';
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
}

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
