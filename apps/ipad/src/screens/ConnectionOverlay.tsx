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

  // Moving an ongoing session onto a USB cable keeps the surface usable, without an overlay.
  if (state.phase === 'idle' || state.phase === 'connected' || state.switching) return null;

  if (state.phase === 'ended') {
    return null;
  }

  const failed = state.phase === 'failed';
  const pairing = state.phase === 'pairing';
  const title = failed
    ? 'Connexion arrêtée'
    : pairing
      ? 'Confirmez sur le PC'
      : state.phase === 'reconnecting'
        ? 'Reconnexion'
        : 'Connexion sécurisée';

  return (
    <div className="overlay" role={failed ? 'alertdialog' : 'dialog'} aria-modal="true" aria-live="polite">
      <div className="overlay-card">
        {failed ? <Icon name="alert" size={34} style={{ color: 'var(--vp-red)' }} /> : pairing ? <div className="rings"><div className="rings-core"><Icon name="shield" size={30} /></div></div> : <Spinner size={28} />}
        <h2>{title}</h2>
        {pairing && state.sas ? (
          <>
            <p>Vérifiez que ce code est le même que celui affiché par Record OS sur Windows, puis autorisez la demande sur le PC.</p>
            <div className="sas" aria-label={`Security code ${state.sas}`}>{state.sas}</div>
          </>
        ) : (
          <p>{failed ? state.error : state.phase === 'reconnecting' ? `Essai ${state.reconnectAttempt} sur 8. Les commandes reprennent dès que la connexion chiffrée revient.` : 'Recherche du chemin privé le plus rapide vers votre ordinateur.'}</p>
        )}
        <Button variant={failed ? 'primary' : 'default'} block onClick={close}>{failed ? 'Retour aux ordinateurs' : 'Annuler'}</Button>
      </div>
    </div>
  );
}
