import { Button, Icon, IconButton, formatDuration } from '@vision-pro/design';
import { useEffect, useRef, useState } from 'react';
import type { ConnectInfo, Device, PairingRequest } from './api.ts';

export function ConnectDialog({ info, request, paired, onRespond, onRefresh, onClose }: {
  info: ConnectInfo;
  request?: PairingRequest;
  paired?: Device;
  onRespond: (id: string, approve: boolean) => Promise<void>;
  onRefresh: () => Promise<void>;
  onClose: () => void;
}) {
  const [now, setNow] = useState(Date.now);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const inFlight = useRef(false);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  useEffect(() => { setError(''); }, [request?.id]);
  const remaining = info.expiresAt - now;
  const pending = request && request.expiresAt > now ? request : undefined;
  const respond = async (approve: boolean) => {
    if (!pending || inFlight.current) return;
    inFlight.current = true;
    setBusy(true); setError('');
    try { await onRespond(pending.id, approve); }
    catch (e) { setError(e instanceof Error ? e.message : 'Impossible de répondre. Réessayez.'); }
    finally { inFlight.current = false; setBusy(false); }
  };
  const renew = async () => {
    if (inFlight.current) return;
    inFlight.current = true; setBusy(true); setError('');
    try { await onRefresh(); }
    catch { setError('Impossible de créer un code. Réessayez.'); }
    finally { inFlight.current = false; setBusy(false); }
  };

  return <div className="dialog-scrim"><section className="connect-dialog" role="dialog" aria-modal="true" aria-labelledby="connect-title">
    <div className="connect-head"><div><p className="eyebrow">Association sécurisée</p><h2 id="connect-title">Connecter un iPad</h2></div><IconButton icon="close" label="Fermer" onClick={onClose} /></div>
    <ol className="connect-steps">
      <li><span className="step-n">1</span><div><b>Ouvrez cette adresse sur l’iPad</b><div className="connect-addresses">{info.addresses.map(address => <div className="connect-address" key={address.url}><small>Connexion par Internet</small><code>{address.url}</code></div>)}</div><small className="connect-note">Aucun compte ni certificat à installer.</small></div></li>
      <li><span className="step-n">2</span><div><b>Saisissez ce code sur l’iPad</b><div className="pair-code" aria-label={`Code ${info.code}`}><b>{info.code.slice(0, 2)} {info.code.slice(2)}</b><span>{pending ? 'Demande reçue' : remaining > 0 ? `expire dans ${formatDuration(remaining)}` : 'Code expiré'}</span></div></div></li>
      <li className="connect-confirm"><span className="step-n">3</span><div aria-live="polite">
        <b>Autorisez votre iPad ici</b>
        {pending ? <>
          <p className="connect-note"><strong>{pending.deviceName}</strong> souhaite se connecter. Vérifiez que le code de comparaison ci-dessous correspond à celui affiché sur l’iPad.</p>
          <div className="pair-code"><b className="comparison-code" aria-label={`Code de comparaison ${pending.sas}`}>{pending.sas}</b></div>
          <div className="connect-decisions"><Button variant="danger" disabled={busy} onClick={() => void respond(false)}>Refuser</Button><Button variant="primary" loading={busy} disabled={busy} onClick={() => void respond(true)}>Associer cet iPad</Button></div>
        </> : paired ? <div className="connect-success" role="status"><Icon name="check" /><span><b>{paired.name}</b> est associé et mémorisé. La prochaine fois, sélectionnez simplement cet ordinateur sur l’iPad.</span></div> : <p className="connect-note">En attente de votre iPad. Sa demande apparaîtra ici après la saisie du code.</p>}
      </div></li>
    </ol>
    {error && <p className="connect-warning" role="alert">{error}</p>}
    <div className="connect-actions">{!pending && <Button icon="refresh" disabled={busy} onClick={() => void renew()}>Nouveau code</Button>}{paired && !pending && <Button variant="primary" onClick={onClose}>Terminé</Button>}</div>
    <div className="trust-note"><Icon name="shield" /><span>L’iPad conserve sa clé de connexion. Vous pouvez retirer son autorisation à tout moment depuis les appareils du PC.</span></div>
  </section></div>;
}
