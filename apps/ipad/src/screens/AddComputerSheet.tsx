import { Button, Sheet } from '@vision-pro/design';
import { PORTS, relayConnectUrl } from '@vision-pro/protocol';
import { useState } from 'react';
import { connection } from '../lib/connection.ts';
import { store, useApp } from '../lib/store.ts';
import { CodeEntry } from './CodeEntry.tsx';

/**
 * Where to go for an address typed from another PC's screen. Without a port it is the setup page
 * shown by "Connect Device" (it installs the trust profile once, then opens the app).
 */
export function addressUrl(input: string): string | null {
  const host = input.trim().replace(/^https?:\/\//i, '').replace(/\/.*$/, '');
  const match = /^([A-Za-z0-9.-]+)(?::(\d{2,5}))?$/.exec(host);
  if (!match) return null;
  const [, name, port] = match;
  if (!port) return `http://${name}:${PORTS.bootstrap}/`;
  return Number(port) === PORTS.https ? `https://${name}:${port}/` : `http://${name}:${port}/`;
}

export function AddComputerSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [address, setAddress] = useState('');
  const target = addressUrl(address);
  const agent = useApp((s) => s.agent);
  const [agentId, setAgentId] = useState('');
  const [fingerprint, setFingerprint] = useState('');
  const [error, setError] = useState('');
  const remotePair = (code: string) => {
    const id = agentId.trim(); const pin = fingerprint.trim();
    if (!/^[A-Za-z0-9_-]{22}$/.test(id) || !/^[A-Za-z0-9_-]{43}$/.test(pin)) {
      setError('Recopiez l’identifiant et l’empreinte du PC depuis Vision-Pro → À propos.'); return;
    }
    const { identity, prefs } = store.get();
    const url = relayConnectUrl(`wss://${location.host}`, id);
    if (!identity || !url) return;
    setError(''); onClose();
    void connection.connect({ agentId: id, name: 'Mon PC', fingerprint: pin, url, mode: 'pair', proof: { kind: 'code', secret: code } }, identity, prefs.deviceName);
  };

  return (
    <Sheet open={open} title="Autre ordinateur" onClose={onClose} width={480}>
      {!agent && <div style={{ display: 'grid', gap: 14, marginBottom: 28 }}>
        <p className="vp-caption">Sur le PC, activez l’accès à distance avec l’adresse de ce site. Dans « À propos », recopiez l’identifiant et l’empreinte, puis choisissez « Connecter un appareil » pour afficher le code.</p>
        <label>Identifiant du PC<input className="vp-input" value={agentId} onChange={(e) => setAgentId(e.currentTarget.value)} autoCapitalize="off" autoCorrect="off" spellCheck={false} maxLength={22} /></label>
        <label>Empreinte de sécurité du PC<input className="vp-input" value={fingerprint} onChange={(e) => setFingerprint(e.currentTarget.value)} autoCapitalize="off" autoCorrect="off" spellCheck={false} maxLength={43} /></label>
        <p className="vp-caption">Code à 6 chiffres affiché sur le PC</p>
        <CodeEntry onSubmit={remotePair} />
        {error && <p role="alert">{error}</p>}
      </div>}
      <p className="vp-caption" style={{ margin: '0 0 14px', fontSize: 15, lineHeight: 1.5 }}>
        Sur l’autre PC, ouvrez Vision-Pro et choisissez <b style={{ color: 'var(--vp-text)' }}>Connecter un appareil</b>. Tapez ici l’adresse affichée, puis le code.
      </p>
      <div style={{ display: 'flex', gap: 8 }}>
        <input
          className="vp-input"
          placeholder="192.168.1.20"
          inputMode="url"
          value={address}
          onChange={(e) => setAddress(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && target) location.href = target;
          }}
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
        />
        <Button variant="primary" disabled={!target} onClick={() => target && (location.href = target)}>
          Ouvrir
        </Button>
      </div>
    </Sheet>
  );
}
