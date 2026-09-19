import { Button, Icon } from '@vision-pro/design';

/** Shown when the app is opened without HTTPS: keys and encryption would not be available. */
export function Insecure() {
  const secureUrl = `https://${location.hostname}:47800/`;
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
            Vision-Pro only runs over an encrypted connection, so your PC and this iPad can verify each other. Open the secure address shown on your PC.
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
