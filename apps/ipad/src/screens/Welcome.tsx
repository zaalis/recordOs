import { Button, Icon } from '@vision-pro/design';
import { useState } from 'react';
import { setScreen, updatePrefs, useApp } from '../lib/store.ts';
import { AccountSheet } from './AccountSheet.tsx';

export function Welcome() {
  const initialName = useApp((s) => s.prefs.deviceName);
  const [name, setName] = useState(initialName === 'iPad' ? '' : initialName);
  const [account, setAccount] = useState<'signin' | 'signup' | null>(null);

  const start = () => {
    updatePrefs({ deviceName: name.trim().slice(0, 48) || 'iPad', onboarded: true });
    setScreen('home');
  };

  return (
    <div className="page">
      <div className="ambient" />
      <div className="page-inner welcome">
        <header className="topbar">
          <span className="wordmark">Vision-Pro</span>
          <Button variant="ghost" size="sm" onClick={() => setAccount('signin')}>
            Sign in
          </Button>
        </header>

        <section className="hero">
          <h1 className="vp-display hero-title vp-animate-in">
            Your computer.
            <span className="dim">Everywhere.</span>
          </h1>
          <p className="hero-subtitle vp-animate-in" style={{ animationDelay: '60ms' }}>
            Turn this iPad into a precise, low-latency interface for your Mac — with touch, Apple Pencil and shortcuts built for creative work.
          </p>
        </section>

        <div className="welcome-form vp-animate-in" style={{ animationDelay: '120ms' }}>
          <label className="vp-overline" htmlFor="device-name">
            Name this iPad
          </label>
          <input
            id="device-name"
            className="vp-input"
            placeholder="Studio iPad"
            value={name}
            maxLength={48}
            autoComplete="off"
            autoCorrect="off"
            onChange={(e) => setName(e.currentTarget.value)}
            onKeyDown={(e) => e.key === 'Enter' && start()}
          />
          <Button variant="primary" size="lg" block onClick={start}>
            Get Started
          </Button>
          <div className="welcome-actions">
            <Button variant="ghost" size="sm" onClick={() => setAccount('signin')}>
              Sign in
            </Button>
            <Button variant="ghost" size="sm" onClick={() => setAccount('signup')}>
              Create account
            </Button>
          </div>
          <div className="trust-line">
            <Icon name="shield" size={16} />
            <span>End-to-end encrypted. No account needed on your own network.</span>
          </div>
        </div>
      </div>
      <AccountSheet mode={account} onClose={() => setAccount(null)} />
    </div>
  );
}
