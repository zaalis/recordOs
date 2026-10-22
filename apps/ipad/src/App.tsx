import { useEffect, useSyncExternalStore } from 'react';
import { connection } from './lib/connection.ts';
import { rememberComputer, removeComputer, setScreen, store, useApp } from './lib/store.ts';
import { ConnectionOverlay } from './screens/ConnectionOverlay.tsx';
import { Home } from './screens/Home.tsx';
import { Insecure } from './screens/Insecure.tsx';
import { Welcome } from './screens/Welcome.tsx';
import { Session } from './stage/Session.tsx';
import { useLocalizedInterface } from './lib/i18n.ts';

export function App() {
  useLocalizedInterface();
  const screen = useApp((s) => s.screen);
  const phase = useSyncExternalStore(connection.subscribe, () => connection.getSnapshot().phase);

  useEffect(() => {
    const onPagehide = () => connection.suspendForPagehide();
    const onVisible = () => { if (!document.hidden) connection.resumeVisible(); };
    window.addEventListener('pagehide', onPagehide);
    window.addEventListener('pageshow', onVisible);
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      window.removeEventListener('pagehide', onPagehide);
      window.removeEventListener('pageshow', onVisible);
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, []);

  // Remember and pin every PC this iPad successfully connects to.
  useEffect(() => {
    connection.onPaired = ({ agentId, name, fingerprint, relayUrl, usbUrls }) => {
      const { agent, computers } = store.get();
      const existing = computers.find((c) => c.agentId === agentId);
      void rememberComputer({
        agentId,
        name,
        fingerprint: fingerprint || existing?.fingerprint || '',
        hostname: agent?.agentId === agentId ? agent.hostname : (existing?.hostname ?? location.hostname),
        port: agent?.agentId === agentId ? agent.port : (existing?.port ?? Number(location.port || 443)),
        pairedAt: existing?.pairedAt ?? Date.now(),
        lastConnectedAt: Date.now(),
        // Every session reports the PC's current relay; an absent value means remote access is off.
        ...(relayUrl ? { relayUrl } : {}),
        // Cable endpoints stay valid across sessions (the tablet gives the PC the same address).
        ...(usbUrls.length ? { usbUrls } : existing?.usbUrls ? { usbUrls: existing.usbUrls } : {}),
      });
    };
    connection.onAccessRemoved = (agentId) => {
      void removeComputer(agentId);
    };
    return () => {
      connection.onPaired = undefined;
      connection.onAccessRemoved = undefined;
    };
  }, []);

  useEffect(() => {
    if (phase === 'connecting' || phase === 'pairing' || phase === 'connected' || phase === 'reconnecting') {
      if (store.get().screen !== 'session') setScreen('session');
    }
  }, [phase]);

  return (
    <div className="app">
      {screen === 'insecure' && <Insecure />}
      {screen === 'welcome' && <Welcome />}
      {screen === 'home' && <Home />}
      {screen === 'session' && <Session />}
      <ConnectionOverlay />
    </div>
  );
}
