import { Button, Icon } from '@vision-pro/design';
import { PORTS } from '@vision-pro/protocol';

/** Shown when the app is opened without HTTPS: keys and encryption would not be available. */
export function Insecure() {
  const secureUrl = `https://${location.hostname}:${PORTS.https}/`;
  return (
    <div className="page">
      <div className="ambient" />
      <div className="page-inner" style={{ justifyContent: 'center', maxWidth: 560 }}>
        <div className="vp-animate-in" style={{ display: 'flex', flexDirection: 'column', gap: 18 }}>
          <div className="computer-glyph" style={{ width: 64, height: 64, borderRadius: 20 }}>
            <Icon name="lock" size={30} />
          </div>
          <h1 className="vp-display" style={{ fontSize: 40 }}>
            Secure connection required
          </h1>
          <p className="hero-subtitle" style={{ marginTop: 0 }}>
            Record OS fonctionne uniquement par une connexion chiffrée, pour que le PC et cet iPad se vérifient. Ouvrez l’adresse affichée par « Connecter un appareil » sur le PC.
          </p>
          <div>
            <Button variant="primary" size="lg" onClick={() => (location.href = secureUrl)}>
              Open secure address
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
