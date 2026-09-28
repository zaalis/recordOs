import { Button, Icon, Sheet } from '@vision-pro/design';
import { parsePairingFragment } from '@vision-pro/protocol';
import jsQR from 'jsqr';
import { useEffect, useRef, useState } from 'react';
import { connection } from '../lib/connection.ts';
import { store } from '../lib/store.ts';

function isPrivateBootstrapHost(host: string) {
  const octets = host.split('.').map(Number);
  const [first, second] = octets;
  return octets.length === 4
    && octets.every((part) => Number.isInteger(part) && part >= 0 && part <= 255)
    && (first === 10 || first === 127 || (first === 192 && second === 168) || (first === 172 && second !== undefined && second >= 16 && second <= 31) || (first === 169 && second === 254));
}

/** In-app QR scanner. The camera stream never leaves the iPad and stops when the sheet closes. */
export function ScanSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    let stream: MediaStream | null = null;
    let frame = 0;
    let stopped = false;
    const canvas = document.createElement('canvas');
    const context = canvas.getContext('2d', { willReadFrequently: true });
    setError(null);

    const handle = (text: string) => {
      let url: URL;
      try {
        url = new URL(text);
      } catch {
        return false;
      }
      const link = parsePairingFragment(url.hash);
      const isSecureApp = url.protocol === 'https:';
      // The desktop shows the short-lived HTTP bootstrap QR on purpose: it lets
      // iPadOS install this PC's local certificate before opening its HTTPS app.
      // The bootstrap listener normally uses 47801, but it can intentionally
      // choose another free local port if a different program owns that port.
      const isLocalBootstrap = url.protocol === 'http:' && !!url.port && isPrivateBootstrapHost(url.hostname) && (url.pathname === '/' || url.pathname === '');
      if (!link || (!isSecureApp && !isLocalBootstrap)) {
        setError('Ce code QR n’est pas un code d’association Vision-Pro valide. Ouvrez « Connecter un appareil » sur le PC et scannez le code affiché.');
        return true;
      }
      if (url.host !== location.host) {
        location.href = url.toString();
        return true;
      }
      const { identity, prefs } = store.get();
      if (!identity) return false;
      onClose();
      void connection.connect(
        { agentId: link.agentId, name: link.name || 'PC', fingerprint: link.fingerprint, mode: 'pair', proof: { kind: 'qr', secret: link.token } },
        identity,
        prefs.deviceName,
        prefs.video,
      );
      return true;
    };

    const scan = () => {
      if (stopped) return;
      const video = videoRef.current;
      if (video && context && video.readyState >= 2) {
        const size = 480;
        const scale = size / Math.max(video.videoWidth, video.videoHeight);
        canvas.width = Math.round(video.videoWidth * scale);
        canvas.height = Math.round(video.videoHeight * scale);
        context.drawImage(video, 0, 0, canvas.width, canvas.height);
        const image = context.getImageData(0, 0, canvas.width, canvas.height);
        const result = jsQR(image.data, image.width, image.height, { inversionAttempts: 'dontInvert' });
        if (result?.data && handle(result.data)) return;
      }
      frame = requestAnimationFrame(scan);
    };

    navigator.mediaDevices
      ?.getUserMedia({ video: { facingMode: 'environment', width: { ideal: 1280 } }, audio: false })
      .then((s) => {
        if (stopped) {
          s.getTracks().forEach((t) => t.stop());
          return;
        }
        stream = s;
        if (videoRef.current) {
          videoRef.current.srcObject = s;
          void videoRef.current.play();
        }
        frame = requestAnimationFrame(scan);
      })
      .catch(() => setError('L’accès à la caméra est nécessaire pour scanner. Vous pouvez aussi utiliser l’appareil photo de l’iPad.'));

    return () => {
      stopped = true;
      cancelAnimationFrame(frame);
      stream?.getTracks().forEach((t) => t.stop());
    };
  }, [open, onClose]);

  return (
    <Sheet open={open} title="Scanner le code QR" onClose={onClose} width={460}>
      <p className="vp-caption" style={{ margin: '0 0 14px', fontSize: 15 }}>
        Sur votre PC, choisissez <b style={{ color: 'var(--vp-text)' }}>Connecter un appareil</b>, puis cadrez le code avec cet iPad.
      </p>
      {error ? (
        <div className="empty-state" role="alert">
          <Icon name="camera" size={30} />
          <p style={{ marginBottom: 0 }}>{error}</p>
        </div>
      ) : (
        <div className="qr-frame">
          <video ref={videoRef} className="qr-video" playsInline muted />
        </div>
      )}
      <div style={{ marginTop: 16 }}>
        <Button block onClick={onClose}>
          Annuler
        </Button>
      </div>
    </Sheet>
  );
}
