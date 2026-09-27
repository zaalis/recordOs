import { Badge, Button, Icon, IconButton, formatRelative, useToast } from '@vision-pro/design';
import { useEffect, useMemo, useState } from 'react';
import { connection } from '../lib/connection.ts';
import { refreshNearby, store, useApp } from '../lib/store.ts';
import type { KnownComputer } from '../lib/storage.ts';
import { AddComputerSheet } from './AddComputerSheet.tsx';
import { DeviceSettingsSheet } from './DeviceSettingsSheet.tsx';
import { ScanSheet } from './ScanSheet.tsx';

interface ComputerEntry {
  agentId: string;
  name: string;
  status: 'here' | 'nearby' | 'paired';
  known?: KnownComputer;
  hostname?: string;
  port?: number;
}

export function Home() {
  const agent = useApp((s) => s.agent);
  const nearby = useApp((s) => s.nearby);
  const computers = useApp((s) => s.computers);
  const [sheet, setSheet] = useState<'add' | 'scan' | 'settings' | null>(null);
  const toast = useToast();

  useEffect(() => {
    const timer = window.setInterval(() => void refreshNearby(), 5000);
    return () => window.clearInterval(timer);
  }, []);

  const entries = useMemo<ComputerEntry[]>(() => {
    const list: ComputerEntry[] = [];
    if (agent) {
      list.push({ agentId: agent.agentId, name: agent.name, status: 'here', known: computers.find((c) => c.agentId === agent.agentId) });
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
  }, [agent, nearby, computers]);

  const connect = (entry: ComputerEntry) => {
    const { identity, prefs } = store.get();
    if (!identity) return;
    if (entry.status !== 'here') {
      // Another Mac hosts its own secure app origin: continue there.
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
    void connection.connect(
      known?.fingerprint
        ? { agentId: entry.agentId, name: entry.name, fingerprint: known.fingerprint, mode: 'auth' }
        : { agentId: entry.agentId, name: entry.name, mode: 'pair', ...(agent?.fingerprint ? { fingerprint: agent.fingerprint } : {}) },
      identity,
      prefs.deviceName,
      prefs.video,
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

        <div className="section-header">
          <h2>Nearby computers</h2>
          <IconButton icon="refresh" label="Refresh" size="sm" onClick={() => void refreshNearby()} />
        </div>

        {entries.length === 0 ? (
          <div className="empty-state vp-animate-in">
            <Icon name="computer" size={34} />
            <h3>No computers found</h3>
            <p style={{ margin: '0 auto', maxWidth: 420, lineHeight: 1.5 }}>
              Open Vision-Pro on your Mac and choose Connect Device, then scan the QR code with this iPad.
            </p>
          </div>
        ) : (
          <div className="computers">
            {entries.map((entry, index) => (
              <article key={entry.agentId} className="computer-card" style={{ animationDelay: `${index * 50}ms` }}>
                <div className="computer-card-head">
                  <div className="computer-glyph">
                    <Icon name="computer" size={26} />
                  </div>
                  <div style={{ minWidth: 0, flex: 1 }}>
                    <div className="computer-name">{entry.name}</div>
                    <div className="computer-meta">
                      {entry.status === 'here' && (
                        <>
                          <span className="vp-dot" data-tone="green" />
                          {entry.known ? 'Ready' : 'Ready to pair'}
                        </>
                      )}
                      {entry.status === 'nearby' && (
                        <>
                          <span className="vp-dot" data-tone="green" />
                          On this network
                        </>
                      )}
                      {entry.status === 'paired' && <>Last used {formatRelative(entry.known?.lastConnectedAt)}</>}
                    </div>
                  </div>
                  {entry.known && <Badge>Paired</Badge>}
                </div>
                <Button variant={entry.status === 'here' ? 'primary' : 'default'} block onClick={() => connect(entry)}>
                  {entry.status === 'here' ? 'Connect' : 'Open'}
                </Button>
              </article>
            ))}
          </div>
        )}

        <div className="home-actions">
          <button type="button" className="action-tile" onClick={() => setSheet('add')}>
            <Icon name="plus" />
            <span>
              Add computer
              <small>Enter the code shown on your Mac</small>
            </span>
          </button>
          <button type="button" className="action-tile" onClick={() => setSheet('scan')}>
            <Icon name="qr" />
            <span>
              Scan QR Code
              <small>Pair in one step</small>
            </span>
          </button>
        </div>

        <footer className="home-footer">
          <span style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <Icon name="lock" size={15} /> Direct, end-to-end encrypted connection
          </span>
          <span>Vision-Pro {agent?.version ?? ''}</span>
        </footer>
      </div>

      <AddComputerSheet open={sheet === 'add'} onClose={() => setSheet(null)} />
      <ScanSheet open={sheet === 'scan'} onClose={() => setSheet(null)} />
      <DeviceSettingsSheet open={sheet === 'settings'} onClose={() => setSheet(null)} />
    </div>
  );
}
