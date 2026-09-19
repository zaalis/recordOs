import { Button, Sheet } from '@vision-pro/design';
import { useEffect, useRef, useState } from 'react';
import { connection } from '../lib/connection.ts';
import { store, useApp } from '../lib/store.ts';

export function AddComputerSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const agent = useApp((s) => s.agent);
  const [digits, setDigits] = useState<string[]>(Array(6).fill(''));
  const [address, setAddress] = useState('');
  const refs = useRef<(HTMLInputElement | null)[]>([]);

  useEffect(() => {
    if (open) {
      setDigits(Array(6).fill(''));
      window.setTimeout(() => refs.current[0]?.focus(), 250);
    }
  }, [open]);

  const code = digits.join('');

  const setDigit = (index: number, value: string) => {
    const clean = value.replace(/\D/g, '');
    if (clean.length > 1) {
      const next = clean.slice(0, 6).split('');
      setDigits([...next, ...Array(6 - next.length).fill('')]);
      refs.current[Math.min(5, next.length)]?.focus();
      return;
    }
    const next = [...digits];
    next[index] = clean;
    setDigits(next);
    if (clean && index < 5) refs.current[index + 1]?.focus();
  };

  const submit = () => {
    const { identity, prefs } = store.get();
    if (!identity || !agent || code.length !== 6) return;
    onClose();
    void connection.connect(
      { agentId: agent.agentId, name: agent.name, fingerprint: agent.fingerprint, mode: 'pair', proof: { kind: 'code', secret: code } },
      identity,
      prefs.deviceName,
      prefs.video,
    );
  };

  const openAddress = () => {
    const host = address.trim().replace(/^https?:\/\//, '').replace(/\/.*$/, '');
    if (!/^[A-Za-z0-9.-]+(:\d{2,5})?$/.test(host)) return;
    location.href = `https://${host.includes(':') ? host : `${host}:47800`}/`;
  };

  return (
    <Sheet open={open} title="Add computer" onClose={onClose} width={480}>
      {agent ? (
        <>
          <p className="vp-caption" style={{ margin: '0 0 8px', fontSize: 15, lineHeight: 1.5 }}>
            On <b style={{ color: 'var(--vp-text)' }}>{agent.name}</b>, open Vision-Pro and choose <b style={{ color: 'var(--vp-text)' }}>Connect Device</b>. Enter the 6-digit code.
          </p>
          <div className="code-inputs">
            {digits.map((digit, index) => (
              <input
                key={index}
                ref={(el) => {
                  refs.current[index] = el;
                }}
                inputMode="numeric"
                autoComplete={index === 0 ? 'one-time-code' : 'off'}
                aria-label={`Digit ${index + 1}`}
                value={digit}
                maxLength={6}
                onChange={(e) => setDigit(index, e.currentTarget.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Backspace' && !digit && index > 0) refs.current[index - 1]?.focus();
                  if (e.key === 'Enter') submit();
                }}
              />
            ))}
          </div>
          <Button variant="primary" size="lg" block disabled={code.length !== 6} onClick={submit}>
            Connect
          </Button>
        </>
      ) : (
        <p className="vp-caption">Open this app from the address shown on your PC to add it.</p>
      )}
      <div style={{ marginTop: 26, display: 'flex', flexDirection: 'column', gap: 10 }}>
        <div className="vp-overline">Another computer</div>
        <div style={{ display: 'flex', gap: 8 }}>
          <input className="vp-input" placeholder="vp-1a2b3c4d.local" value={address} onChange={(e) => setAddress(e.currentTarget.value)} autoCapitalize="off" autoCorrect="off" />
          <Button onClick={openAddress}>Open</Button>
        </div>
      </div>
    </Sheet>
  );
}
