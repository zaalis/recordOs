import { Button, Group, Icon, Row, Sheet, Spinner, useToast } from '@vision-pro/design';
import { useEffect, useState } from 'react';
import { type AccountConfig, account, accountConfig, accountErrorText, AccountError, signInWithPasskey } from '../lib/account.ts';

export function AccountSheet({ mode, onClose }: { mode: 'signin' | 'signup' | null; onClose: () => void }) {
  const [config, setConfig] = useState<AccountConfig | null | undefined>(undefined);
  const [tab, setTab] = useState<'signin' | 'signup'>('signin');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [name, setName] = useState('');
  const [totp, setTotp] = useState('');
  const [needsTotp, setNeedsTotp] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const toast = useToast();

  useEffect(() => {
    if (!mode) return;
    setTab(mode);
    setError(null);
    setNeedsTotp(false);
    void accountConfig().then(setConfig);
  }, [mode]);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      if (tab === 'signup') {
        await account.register(email.trim(), password, name.trim());
        toast('Account created', { icon: 'check-circle', tone: 'green' });
        onClose();
      } else {
        const result = await account.login(email.trim(), password, needsTotp ? totp : undefined);
        if (result.totpRequired) {
          setNeedsTotp(true);
        } else {
          toast('Signed in', { icon: 'check-circle', tone: 'green' });
          onClose();
        }
      }
    } catch (e) {
      setError(accountErrorText(e instanceof AccountError ? e.code : 'unknown'));
    } finally {
      setBusy(false);
    }
  };

  const passkey = async () => {
    setBusy(true);
    setError(null);
    try {
      await signInWithPasskey();
      toast('Signed in with passkey', { icon: 'key', tone: 'green' });
      onClose();
    } catch (e) {
      setError(accountErrorText(e instanceof AccountError ? e.code : 'unknown'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Sheet open={mode !== null} title={tab === 'signup' ? 'Create account' : 'Sign in'} onClose={onClose} width={460}>
      {config === undefined && (
        <div style={{ display: 'grid', placeItems: 'center', padding: 40 }}>
          <Spinner />
        </div>
      )}
      {config === null && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 18 }}>
          <p className="vp-caption" style={{ margin: 0, fontSize: 15, lineHeight: 1.5 }}>
            Accounts are optional. Record OS works completely on your local network: your iPad and PC connect directly and nothing passes through the cloud.
          </p>
          <Group title="An account adds">
            <Row icon="devices" title="All your PCs in one place" subtitle="Find and manage every computer" />
            <Row icon="refresh" title="Synced preferences" subtitle="Shortcuts and quality settings follow you" />
            <Row icon="shield" title="Remote revocation" subtitle="Remove a lost iPad from anywhere" />
          </Group>
          <p className="vp-footnote" style={{ padding: 0 }}>
            This page is served directly by your PC. To use an account, open Record OS from your account server.
          </p>
          <Button variant="primary" block onClick={onClose}>
            Continue without account
          </Button>
        </div>
      )}
      {config && (
        <form
          style={{ display: 'flex', flexDirection: 'column', gap: 12 }}
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          {config.passkeys && tab === 'signin' && (
            <>
              <Button variant="primary" size="lg" block icon="key" onClick={() => void passkey()} disabled={busy}>
                Sign in with a passkey
              </Button>
              <div className="vp-overline" style={{ textAlign: 'center', margin: '6px 0' }}>
                or
              </div>
            </>
          )}
          {tab === 'signup' && <input className="vp-input" placeholder="Name" autoComplete="name" value={name} onChange={(e) => setName(e.currentTarget.value)} maxLength={64} />}
          <input className="vp-input" type="email" placeholder="Email" autoComplete="username" value={email} onChange={(e) => setEmail(e.currentTarget.value)} required maxLength={254} />
          <input
            className="vp-input"
            type="password"
            placeholder={tab === 'signup' ? 'Password (12+ characters)' : 'Password'}
            autoComplete={tab === 'signup' ? 'new-password' : 'current-password'}
            value={password}
            onChange={(e) => setPassword(e.currentTarget.value)}
            required
            minLength={tab === 'signup' ? 12 : 1}
            maxLength={256}
          />
          {needsTotp && (
            <input className="vp-input" inputMode="numeric" placeholder="6-digit verification code" autoComplete="one-time-code" value={totp} onChange={(e) => setTotp(e.currentTarget.value)} maxLength={6} />
          )}
          {error && (
            <div style={{ display: 'flex', gap: 8, alignItems: 'center', color: 'var(--vp-red)', fontSize: 14 }}>
              <Icon name="alert" size={16} /> {error}
            </div>
          )}
          <Button type="submit" variant={config.passkeys && tab === 'signin' ? 'default' : 'primary'} size="lg" block loading={busy} disabled={busy}>
            {tab === 'signup' ? 'Create account' : 'Sign in'}
          </Button>
          {config.registration && (
            <Button variant="ghost" size="sm" onClick={() => setTab(tab === 'signin' ? 'signup' : 'signin')}>
              {tab === 'signin' ? 'New to Record OS? Create an account' : 'Already have an account? Sign in'}
            </Button>
          )}
        </form>
      )}
    </Sheet>
  );
}
