import { Button, Icon, Spinner } from '@vision-pro/design';
import { useEffect, useSyncExternalStore } from 'react';
import { connection } from '../lib/connection.ts';
import { setScreen } from '../lib/store.ts';

export function ConnectionOverlay() {
  const state = useSyncExternalStore(connection.subscribe, connection.getSnapshot);
  const close = () => {
    connection.reset();
    setScreen('home');
  };

  useEffect(() => {
    if (state.phase === 'ended') {
      connection.reset();
      setScreen('home');
    }
  }, [state.phase]);

  if (state.phase === 'idle' || state.phase === 'streaming') return null;

  if (state.phase === 'ended') {
    return null;
  }

  const failed = state.phase === 'failed';
  const pairing = state.phase === 'pairing';
  const title = failed
    ? 'Connection stopped'
    : pairing
      ? 'Confirm on your Mac'
      : state.phase === 'reconnecting'
        ? 'Reconnecting'
        : state.phase === 'negotiating'
          ? 'Starting the display'
          : 'Connecting securely';

  return (
    <div className="overlay" role={failed ? 'alertdialog' : 'dialog'} aria-modal="true" aria-live="polite">
      <div className="overlay-card">
        {failed ? <Icon name="alert" size={34} style={{ color: 'var(--vp-red)' }} /> : pairing ? <div className="rings"><div className="rings-core"><Icon name="shield" size={30} /></div></div> : <Spinner size={28} />}
        <h2>{title}</h2>
        {pairing && state.sas ? (
          <>
            <p>Verify that this code matches the one shown by Vision-Pro on the Mac, then approve the request there.</p>
            <div className="sas" aria-label={`Security code ${state.sas}`}>{state.sas}</div>
          </>
        ) : (
          <p>{failed ? state.error : state.phase === 'reconnecting' ? `Attempt ${state.reconnectAttempt} of 8. Your input is paused until the secure channel returns.` : 'Discovering the fastest private path to your computer.'}</p>
        )}
        <Button variant={failed ? 'primary' : 'default'} block onClick={close}>{failed ? 'Back to computers' : 'Cancel'}</Button>
      </div>
    </div>
  );
}
