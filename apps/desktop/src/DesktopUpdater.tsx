import { isTauri } from '@tauri-apps/api/core';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { Button, Icon } from '@vision-pro/design';
import { useEffect, useRef, useState } from 'react';
import { api } from './api.ts';

type Transfer =
  | { phase: 'idle' }
  | { phase: 'downloading'; version: string; progress?: number }
  | { phase: 'ready' | 'restarting' | 'failed'; version: string };

const CHECK_INTERVAL = 30 * 60_000;

export function DesktopUpdater() {
  const [transfer, setTransfer] = useState<Transfer>({ phase: 'idle' });
  const update = useRef<Update | null>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const installPending = useRef(false);

  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    let checking = false;

    async function download() {
      if (checking || update.current) return;
      checking = true;
      let candidate: Update | null = null;
      try {
        candidate = await check({ timeout: 15_000 });
        if (!candidate || !alive) return;
        const version = candidate.version;
        let total = 0;
        let bytes = 0;
        let lastProgress = -1;
        setTransfer({ phase: 'downloading', version });
        await candidate.download(event => {
          if (event.event === 'Started') total = event.data.contentLength ?? 0;
          if (event.event !== 'Progress' || !alive || !total) return;
          bytes += event.data.chunkLength;
          // Keep the last percent for signature verification.
          const progress = Math.min(99, Math.floor(100 * bytes / total));
          if (progress === lastProgress) return;
          lastProgress = progress;
          setTransfer({ phase: 'downloading', version, progress });
        }, { timeout: 300_000 });
        if (!alive) return;
        update.current = candidate;
        setTransfer({ phase: 'ready', version });
      } catch (error) {
        if (candidate && alive) setTransfer({ phase: 'failed', version: candidate.version });
        console.warn('Vérification de mise à jour Record OS', error);
      } finally {
        if (candidate && candidate !== update.current) await candidate.close().catch(() => {});
        checking = false;
      }
    }

    // Also avoids starting a duplicate request during the StrictMode mount check.
    const startup = window.setTimeout(() => void download(), 1500);
    const interval = window.setInterval(() => void download(), CHECK_INTERVAL);
    return () => {
      alive = false;
      window.clearTimeout(startup);
      window.clearInterval(interval);
      const candidate = update.current;
      update.current = null;
      if (candidate) void candidate.close().catch(() => {});
    };
  }, []);

  async function install() {
    const candidate = update.current;
    if (!candidate || installPending.current) return;
    const version = candidate.version;
    installPending.current = true;
    setTransfer({ phase: 'restarting', version });
    // Give WebView time to paint the restart message before NSIS closes the app.
    await new Promise(resolve => window.setTimeout(resolve, 650));
    let resumeService = false;
    try {
      const status = await api.status();
      resumeService = status.running;
      if (resumeService) await api.service(false);
      await candidate.install({ restartAfterInstall: true });
    } catch (error) {
      if (resumeService) await api.service(true).catch(() => {});
      console.error('Installation de la mise à jour Record OS', error);
      setTransfer({ phase: 'failed', version });
    } finally {
      installPending.current = false;
    }
  }

  if (transfer.phase === 'idle') return null;
  const { phase, version } = transfer;
  const progress = phase === 'downloading' ? transfer.progress : phase === 'failed' ? undefined : 100;
  return <>
    <button className="update-trigger" type="button" onClick={() => {
      dialog.current?.showModal();
      if (phase === 'ready') void install();
    }}>
      <span className="update-symbol" aria-hidden="true">{phase === 'downloading' ? <span className="vp-spinner" /> : <Icon name={phase === 'failed' ? 'alert' : 'update'} size={17} />}</span>
      <span>{phase === 'downloading' ? 'Téléchargement' : phase === 'failed' ? 'Mise à jour en attente' : 'Mise à jour'}<small>Record OS {version}{phase === 'downloading' && progress !== undefined ? ' · ' + progress + ' %' : ''}</small></span>
    </button>
    <dialog ref={dialog} className="connect-dialog update-dialog" aria-labelledby="update-title" onCancel={event => { if (phase === 'restarting') event.preventDefault(); }}>
      <h2 id="update-title">Mise à jour de Record OS</h2>
      <p role="status">{phase === 'downloading' ? 'Téléchargement des fichiers…' : phase === 'ready' ? 'Fichiers téléchargés et vérifiés.' : phase === 'restarting' ? 'Redémarrage de votre application…' : update.current ? 'L’installation a échoué. Vous pouvez réessayer.' : 'Le téléchargement a échoué. Record OS réessaiera automatiquement.'}</p>
      {phase !== 'failed' && <progress value={progress} max={100} aria-label="Progression de la mise à jour" />}
      {phase !== 'restarting' && <div className="update-actions">
        <Button onClick={() => dialog.current?.close()}>Fermer</Button>
        {(phase === 'ready' || phase === 'failed' && update.current) &&
          <Button variant="primary" onClick={() => void install()}>Mettre à jour</Button>}
      </div>}
    </dialog>
  </>;
}
