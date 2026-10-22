import { Badge, Button, Icon, IconButton, formatRelative, useToast } from '@vision-pro/design';
import { cableSignalingUrls, firstReachable, relayConnectUrl } from '@vision-pro/protocol';
import { useEffect, useMemo, useState } from 'react';
import { connection } from '../lib/connection.ts';
import { cableEndpoints, refreshNearby, store, useApp } from '../lib/store.ts';
import type { KnownComputer } from '../lib/storage.ts';
import { AddComputerSheet } from './AddComputerSheet.tsx';
import { CodeEntry } from './CodeEntry.tsx';
import { DeviceSettingsSheet } from './DeviceSettingsSheet.tsx';

interface ComputerEntry {
  agentId: string;
  name: string;
  status: 'here' | 'nearby' | 'paired' | 'usb';
  known?: KnownComputer;
  hostname?: string;
  port?: number;
  /** Cable endpoint this iPad reached (it is the tablet plugged into this PC). */
  cable?: string;
}

/**
 * Finds the PCs this iPad reaches through a USB cable. With Wi-Fi, only when the PC reports a
 * cable; without Wi-Fi, every few seconds from the endpoints learned earlier.
 */
function useCablePaths(): Record<string, string> {
  const agent = useApp((s) => s.agent);
  const computers = useApp((s) => s.computers);
  const [found, setFound] = useState<Record<string, string>>({});
  const announced = JSON.stringify(agent?.usb?.urls ?? []);
  const known = computers.map((c) => c.agentId).join();
  useEffect(() => {
    let cancelled = false;
    const probe = async () => {
      const next: Record<string, string> = {};
      if (agent) {
        const url = await firstReachable(cableSignalingUrls(agent.usb?.urls), agent.agentId);
        if (url) next[agent.agentId] = url;
      } else {
        await Promise.all(
          store.get().computers.map(async (computer) => {
            const url = await firstReachable(await cableEndpoints(computer.agentId, computer.usbUrls ?? []), computer.agentId);
            if (url) next[computer.agentId] = url;
          }),
        );
      }
      if (!cancelled) setFound(next);
    };
    void probe();
    // Without the app origin the cable can only be found by looking again.
    const timer = agent ? undefined : window.setInterval(() => void probe(), 6000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
    // `announced` and `known` capture the parts of `agent` and `computers` that matter here.
  }, [agent?.agentId, announced, known]);
  return found;
}

export function Home() {
  const agent = useApp((s) => s.agent);
  const nearby = useApp((s) => s.nearby);
  const computers = useApp((s) => s.computers);
  const [sheet, setSheet] = useState<'add' | 'settings' | null>(null);
  const cable = useCablePaths();
  const toast = useToast();

  useEffect(() => {
    const timer = window.setInterval(() => void refreshNearby(), 5000);
    return () => window.clearInterval(timer);
  }, []);

  const entries = useMemo<ComputerEntry[]>(() => {
    const list: ComputerEntry[] = [];
    if (agent) {
      list.push({ agentId: agent.agentId, name: agent.name, status: 'here', known: computers.find((c) => c.agentId === agent.agentId), cable: cable[agent.agentId] });
    }
    // Reached through the USB cable only (no Wi-Fi): connect directly over the cable.
    for (const c of computers) {
      if (list.some((e) => e.agentId === c.agentId) || !cable[c.agentId]) continue;
      list.push({ agentId: c.agentId, name: c.name, status: 'usb', known: c, cable: cable[c.agentId], hostname: c.hostname, port: c.port });
    }
    for (const n of nearby) {
      if (list.some((e) => e.agentId === n.agentId)) continue;
      list.push({ agentId: n.agentId, name: n.name, status: 'nearby', hostname: n.hostname, port: n.port, known: computers.find((c) => c.agentId === n.agentId) });
    }
    for (const c of computers) {
      if (list.some((e) => e.agentId === c.agentId)) continue;
      list.push({ agentId: c.agentId, name: c.name, status: 'paired', known: c, hostname: c.hostname, port: c.port });
    }
    return list;
  }, [agent, nearby, computers, cable]);

  /** First connection to the PC serving this app: the code shown on its screen pairs this iPad. */
  const pair = (code: string) => {
    const { identity, prefs } = store.get();
    if (!identity || !agent) return;
    void connection.connect(
      { agentId: agent.agentId, name: agent.name, fingerprint: agent.fingerprint, mode: 'pair', proof: { kind: 'code', secret: code }, ...(cable[agent.agentId] ? { usbUrls: [cable[agent.agentId]!] } : {}) },
      identity,
      prefs.deviceName,
    );
  };

  const connect = (entry: ComputerEntry) => {
    const { identity, prefs } = store.get();
    if (!identity) return;
    if (entry.status === 'usb' && entry.known) {
      // The whole session goes through the cable.
      void connection.connect(
        { agentId: entry.agentId, name: entry.name, fingerprint: entry.known.fingerprint, mode: 'auth', usbUrls: [entry.cable!] },
        identity,
        prefs.deviceName,
      );
      return;
    }
    const relayUrl = entry.status === 'paired' && entry.known?.relayUrl ? relayConnectUrl(entry.known.relayUrl, entry.agentId) : null;
    if (relayUrl && entry.known) {
      // Not on this network: reach the PC through its relay. The pinned key still authenticates it.
      void connection.connect(
        { agentId: entry.agentId, name: entry.name, fingerprint: entry.known.fingerprint, mode: 'auth', url: relayUrl },
        identity,
        prefs.deviceName,
      );
      return;
    }
    if (entry.status !== 'here') {
      // Another PC hosts its own secure app origin: continue there.
      const host = entry.hostname ?? entry.known?.hostname;
      const port = entry.port ?? entry.known?.port ?? 47800;
      if (host) {
        location.href = `https://${host}:${port}/`;
      } else {
        toast('This computer is not reachable from here.', { icon: 'alert', tone: 'orange' });
      }
      return;
    }
    const known = entry.known;
    if (!known?.fingerprint) return;
    void connection.connect(
      { agentId: entry.agentId, name: entry.name, fingerprint: known.fingerprint, mode: 'auth', usbUrls: entry.cable ? [entry.cable] : [] },
      identity,
      prefs.deviceName,
    );
  };

  return (
    <div className="page">
      <div className="ambient" />
      <div className="page-inner">
        <header className="topbar">
          <span className="wordmark">Vision-Pro</span>
          <IconButton icon="gear" label="Settings" onClick={() => setSheet('settings')} />
        </header>

        <section className="hero" style={{ paddingBottom: 36 }}>
          <h1 className="vp-display hero-title vp-animate-in">
            Your computer.
            <span className="dim">Everywhere.</span>
          </h1>
        </section>

        {agent && !entries[0]?.known && (
          <section className="pair-card vp-animate-in" aria-labelledby="pair-title">
            <div className="computer-card-head">
              <div className="computer-glyph">
                <Icon name="computer" size={26} />
              </div>
              <div style={{ minWidth: 0, flex: 1 }}>
                <div className="computer-name" id="pair-title">{agent.name}</div>
                <div className="computer-meta">
                  <span className="vp-dot" data-tone={agent.pairing?.code ? 'green' : 'orange'} />
                  {agent.pairing?.code ? 'Code affiché sur le PC' : 'Sur le PC : Vision-Pro → Connecter un appareil'}
                </div>
              </div>
            </div>
            <p className="pair-help">Entrez le code à 6 chiffres affiché sur l’écran du PC.</p>
            <CodeEntry onSubmit={pair} autoFocus />
          </section>
        )}

        <div className="section-header">
          <h2>Ordinateurs</h2>
          <IconButton icon="refresh" label="Refresh" size="sm" onClick={() => void refreshNearby()} />
        </div>

        {entries.filter((entry) => entry.known || entry.status !== 'here').length === 0 ? (
          <div className="empty-state vp-animate-in">
            <Icon name="computer" size={34} />
            <h3>Aucun ordinateur associé</h3>
            <p style={{ margin: '0 auto', maxWidth: 420, lineHeight: 1.5 }}>
              Sur votre PC Windows, ouvrez Vision-Pro et choisissez « Connecter un appareil ». Ouvrez l’adresse affichée sur cet iPad, puis entrez le code.
            </p>
          </div>
        ) : (
          <div className="computers">
            {entries.filter((entry) => entry.known || entry.status !== 'here').map((entry, index) => (
              <article key={entry.agentId} className="computer-card" style={{ animationDelay: `${index * 50}ms` }}>
                <div className="computer-card-head">
                  <div className="computer-glyph">
                    <Icon name="computer" size={26} />
                  </div>
                  <div style={{ minWidth: 0, flex: 1 }}>
                    <div className="computer-name">{entry.name}</div>
                    <div className="computer-meta">
                      {entry.status === 'here' && entry.cable && (
                        <>
                          <Icon name="cable" size={14} />
                          Câble USB · connexion directe
                        </>
                      )}
                      {entry.status === 'here' && !entry.cable && (
                        <>
                          <span className="vp-dot" data-tone="green" />
                          Prêt
                        </>
                      )}
                      {entry.status === 'usb' && (
                        <>
                          <Icon name="cable" size={14} />
                          Connecté par câble USB
                        </>
                      )}
                      {entry.status === 'nearby' && (
                        <>
                          <span className="vp-dot" data-tone="green" />
                          Sur ce réseau
                        </>
                      )}
                      {entry.status === 'paired' && entry.known?.relayUrl && (
                        <>
                          <Icon name="globe" size={14} />
                          Via Internet
                        </>
                      )}
                      {entry.status === 'paired' && !entry.known?.relayUrl && <>Utilisé {formatRelative(entry.known?.lastConnectedAt)}</>}
                    </div>
                  </div>
                  {entry.cable ? <Badge tone="green" dot>USB</Badge> : entry.known && <Badge>Associé</Badge>}
                </div>
                <Button variant={entry.status === 'here' || entry.status === 'usb' || entry.known?.relayUrl ? 'primary' : 'default'} block onClick={() => connect(entry)}>
                  {entry.status === 'here' || entry.status === 'usb' || (entry.status === 'paired' && entry.known?.relayUrl) ? 'Connecter' : 'Ouvrir'}
                </Button>
              </article>
            ))}
          </div>
        )}

        <div className="home-actions">
          <button type="button" className="action-tile" onClick={() => setSheet('add')}>
            <Icon name="plus" />
            <span>
              Autre ordinateur
              <small>Tapez l’adresse affichée sur cet autre PC</small>
            </span>
          </button>
        </div>

        <footer className="home-footer">
          <span style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <Icon name="lock" size={15} /> End-to-end encrypted connection
          </span>
          <span>Vision-Pro {agent?.version ?? ''}</span>
        </footer>
      </div>

      <AddComputerSheet open={sheet === 'add'} onClose={() => setSheet(null)} />
      <DeviceSettingsSheet open={sheet === 'settings'} onClose={() => setSheet(null)} />
    </div>
  );
}
