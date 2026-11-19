import { Badge, Button, Group, Icon, IconButton, Row, Segmented, Switch, formatMs, formatRelative, useToast, type IconName } from '@vision-pro/design';
import { useCallback, useEffect, useState } from 'react';
import type { RelayStatus, UsbLink, UsbStatus, UsbTablet } from './api.ts';
import { api, type AgentStatus, type AuditEntry, type ConnectAddress, type ConnectInfo, type Device, type Settings } from './api.ts';
import { useDesktopLanguage } from './i18n.ts';
import { version as appVersion } from '../src-tauri/tauri.conf.json';
import logo from '../src-tauri/icons/64x64.png';
import { DesktopUpdater } from './DesktopUpdater.tsx';
import { ConnectDialog } from './ConnectDialog.tsx';

type Page = 'home' | 'devices' | 'display' | 'input' | 'security' | 'network' | 'advanced' | 'about';
const nav: { id: Page; label: string; icon: IconName }[] = [
  { id: 'home', label: 'Overview', icon: 'home' }, { id: 'devices', label: 'Devices', icon: 'devices' }, { id: 'display', label: 'Display', icon: 'display' },
  { id: 'input', label: 'Input', icon: 'pointer' }, { id: 'security', label: 'Security', icon: 'shield' },
  { id: 'network', label: 'Network', icon: 'network' }, { id: 'advanced', label: 'Advanced', icon: 'sliders' }, { id: 'about', label: 'About', icon: 'info' },
];

export function App() {
  useDesktopLanguage();
  const [page, setPage] = useState<Page>('home');
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  const [audit, setAudit] = useState<AuditEntry[]>([]);
  const [connect, setConnect] = useState<ConnectInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const [connectOpenedAt, setConnectOpenedAt] = useState(0);
  const connectOpen = connect !== null;

  const refresh = useCallback(async () => {
    const [nextStatus, nextSettings, nextDevices, nextAudit] = await Promise.all([api.status(), api.settings(), api.devices(), api.audit(100)]);
    setStatus(nextStatus); setSettings(nextSettings); setDevices(nextDevices); setAudit(nextAudit);
  }, []);

  // A code is replaced once it has paired an iPad (or after five wrong tries): show the current one.
  useEffect(() => {
    if (!connectOpen) return;
    const sync = () => void api.openConnect(false).then(setConnect).catch(() => undefined);
    const timer = window.setInterval(sync, 2000);
    let unsubscribe: () => void = () => {};
    void api.onEvent(sync).then((fn) => { unsubscribe = fn; });
    return () => { clearInterval(timer); unsubscribe(); };
  }, [connectOpen]);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1500);
    let unsubscribe: () => void = () => {};
    void api.onEvent(() => void refresh()).then((fn) => { unsubscribe = fn; });
    return () => { clearInterval(timer); unsubscribe(); };
  }, [refresh]);

  const save = async (next: Settings) => {
    setSettings(next);
    try { await api.saveSettings(next); toast('Settings saved.', { icon: 'check', tone: 'green' }); await refresh(); }
    catch (error) { toast(String(error), { icon: 'alert', tone: 'red' }); }
  };

  const service = async () => {
    if (!status) return;
    setBusy(true);
    try { await api.service(!status.running); await refresh(); }
    catch (error) { toast(String(error), { icon: 'alert', tone: 'red' }); }
    finally { setBusy(false); }
  };

  const openConnect = async () => {
    setBusy(true);
    try { setConnect(await api.openConnect(false)); setConnectOpenedAt(Date.now()); }
    catch (error) { toast(String(error), { icon: 'alert', tone: 'red' }); }
    finally { setBusy(false); }
  };

  const allowUsb = async (link: UsbLink) => {
    try { await api.allowUsbLink(link.id); toast('Liaison USB configurée.', { icon: 'check', tone: 'green' }); await refresh(); }
    catch (error) { toast(String(error), { icon: 'alert', tone: 'red' }); }
  };

  if (!status || !settings) return <div className="desktop-loading"><span className="vp-spinner" /><span>Starting Record OS…</span></div>;

  return (
    <div className="desktop-shell">
      <aside className="sidebar">
        <div className="desktop-wordmark"><img src={logo} width={38} height={38} alt="" />Record OS</div>
        <p className="sidebar-label">Espace de travail</p>
        <nav aria-label="Navigation principale">{nav.map((item) => <button key={item.id} type="button" className="nav-item" aria-current={page === item.id ? 'page' : undefined} onClick={() => setPage(item.id)}><Icon name={item.icon} size={19} /><span>{item.label}</span>{item.id === 'devices' && devices.length > 0 && <span className="nav-count">{devices.length}</span>}</button>)}</nav>
        <div className="sidebar-footer">
          <DesktopUpdater />
          <div className="sidebar-status">
            <span className="dashboard-icon"><Icon name="computer" size={19} /></span>
            <div><b>{status.computerName}</b><small>{status.running ? 'Service actif' : 'Service arrêté'}</small>{status.usb.links.length > 0 && <small className="sidebar-usb"><Icon name="cable" size={12} /> Câble USB actif</small>}</div>
          </div>
        </div>
      </aside>

      <main className="desktop-main">
        <header className="desktop-header">
          <div className="desktop-crumb"><span>Record OS</span><Icon name="chevron-right" size={15} /><strong>{nav.find(item => item.id === page)?.label}</strong></div>
          <div className="header-actions">
            <Button onClick={() => void service()} loading={busy}>{status.running ? 'Stop service' : 'Start service'}</Button>
            <Button variant="primary" icon="plus" onClick={() => void openConnect()} disabled={!status.running} loading={busy}>Connect Device</Button>
          </div>
        </header>
        {status.error && <div className="desktop-alert" data-tone="red"><Icon name="alert" /><span>{status.error}</span></div>}
        {status.storeIntegrityWarning && <div className="desktop-alert" data-tone="orange"><Icon name="shield" /><span>The device store failed its integrity check. Every device now requires confirmation.</span></div>}
        {status.pendingPairing && !connect && <PairingBanner request={status.pendingPairing} onRespond={async (approve) => { await api.respondPairing(status.pendingPairing!.id, approve); await refresh(); }} />}

        <div className="desktop-content" key={page}>
          <h1 className="desktop-page-title">{nav.find(item => item.id === page)?.label}</h1>
          {page === 'home' && <Overview status={status} devices={devices} audit={audit} navigate={setPage} onAllowUsb={allowUsb} />}
          {page === 'devices' && <>
            {status.sessions.length > 0 && <div className="device-tools"><Button icon="power" onClick={async () => {
              try {
                await api.emergency();
                await refresh();
                toast('Sessions déconnectées.', { icon: 'check' });
              } catch (error) {
                toast(String(error), { icon: 'alert' });
              }
            }}>Déconnecter tous les appareils</Button></div>}
            <Devices devices={devices} act={async (id, action, value) => { await api.deviceAction(id, action, value); await refresh(); }} />
          </>}
          {page === 'display' && <DisplayPage status={status} settings={settings} save={save} />}
          {page === 'input' && <InputPage settings={settings} save={save} />}
          {page === 'security' && <SecurityPage status={status} settings={settings} save={save} audit={audit} />}
          {page === 'network' && <NetworkPage status={status} settings={settings} save={save} onAllowUsb={allowUsb} />}
          {page === 'advanced' && <AdvancedPage status={status} settings={settings} save={save} />}
          {page === 'about' && <About status={status} />}
        </div>
      </main>
      {connect && <ConnectDialog info={connect} request={status.pendingPairing} onRespond={async (id, approve) => { if (!await api.respondPairing(id, approve)) throw new Error("Cette demande a expiré. Recommencez depuis l’iPad."); await refresh(); }} paired={devices.filter((d) => d.pairedAt >= connectOpenedAt).sort((a, b) => b.pairedAt - a.pairedAt)[0]} onRefresh={async () => setConnect(await api.openConnect(true))} onClose={() => { setConnect(null); void api.closeConnect(); }} />}
    </div>
  );
}

function Overview({ status, devices, audit, navigate, onAllowUsb }: {
  status: AgentStatus;
  devices: Device[];
  audit: AuditEntry[];
  navigate: (page: Page) => void;
  onAllowUsb: (link: UsbLink) => Promise<void>;
}) {
  const session = status.sessions[0];
  const onlineCount = devices.filter(device => device.online).length;
  return <>
    <p className="dashboard-intro">Gérez votre PC et les appareils qui peuvent le contrôler.</p>
    <div className="summary-grid">
      <Summary icon="tablet" label="Appareils" value={String(devices.length)} detail={onlineCount + (onlineCount === 1 ? ' appareil en ligne' : ' appareils en ligne')} onClick={() => navigate('devices')} />
      <Summary icon="gauge" label="Latence" value={session?.rttMs != null ? formatMs(session.rttMs / 2) : '—'} detail={session ? pathLabel(session) : 'Aucune connexion'} onClick={() => navigate('network')} />
    </div>
    {status.usb.enabled && (status.usb.links.length > 0 || status.usb.devices.length > 0) && <UsbCard usb={status.usb} sessions={status.sessions} onAllow={onAllowUsb} />}
    <div className="dashboard-columns">
      <section className="desktop-card">
        <CardTitle title="Vos appareils" action={<Button size="sm" onClick={() => navigate('devices')}>Tout afficher</Button>} />
        {devices.length ? <div className="activity-list">{devices.map(device => (
          <button type="button" className="activity dashboard-device" key={device.id} onClick={() => navigate('devices')}>
            <span className="dashboard-icon"><Icon name="tablet" size={20} /></span>
            <div><b>{device.name}</b><small>{device.permission === 'control' ? 'Contrôle autorisé' : 'Lecture seule'}</small></div>
            <Badge dot>{device.online ? 'En ligne' : 'Hors ligne'}</Badge>
            <Icon name="chevron-right" size={16} />
          </button>
        ))}</div> : <div className="desktop-empty">Aucun appareil associé.</div>}
      </section>
      <section className="desktop-card">
        <CardTitle title="Activité récente" action={<Button size="sm" onClick={() => navigate('security')}>Voir le journal</Button>} />
        {audit.length ? <div className="activity-list">{audit.slice(0, 5).map((entry, i) => (
          <div key={entry.ts + '-' + i} className="activity">
            <span className="dashboard-icon"><Icon name={entry.severity === 'info' ? 'clock' : 'alert'} size={18} /></span>
            <div><b>{entry.message}</b><small>{formatRelative(entry.ts)}</small></div>
          </div>
        ))}</div> : <div className="desktop-empty">Aucun événement enregistré.</div>}
      </section>
    </div>
  </>;
}

function Devices({ devices, act }: { devices: Device[]; act: (id: string, action: string, value?: string) => Promise<void> }) {
  const [pending, setPending] = useState<string | null>(null);
  if (!devices.length) return <Empty icon="devices" title="No paired devices" text="Choose Connect Device to pair an iPad. Discovery never grants access automatically." />;
  const statusText = { trusted: 'De confiance', untrusted: 'À confirmer', revoked: 'Révoqué' } as const;
  return <div className="device-grid">{devices.map((device) => <article key={device.id} className="device-card"><div className="device-top"><span className="device-icon"><Icon name="tablet" /></span><div><h3>{device.name}</h3><p>{device.kind} · {device.lastSeenAt ? `Last connected ${formatRelative(device.lastSeenAt)}` : 'Never connected'}</p></div><Badge tone={device.online ? 'green' : device.status === 'untrusted' ? 'orange' : undefined} dot>{device.online ? 'Online' : statusText[device.status]}</Badge></div><dl className="device-details"><dt>Permission</dt><dd>{device.permission === 'control' ? 'View & Control' : 'View Only'}</dd><dt>Last address</dt><dd>{device.lastIp ?? '—'}</dd><dt>Key</dt><dd className="mono">{device.fingerprintDisplay || 'Unavailable'}</dd></dl><div className="device-actions">{device.status === 'untrusted' && <Button size="sm" onClick={() => void act(device.id, 'trust')}>Trust Device</Button>}<Button size="sm" onClick={() => { const name = prompt('Device name', device.name); if (name) void act(device.id, 'rename', name); }}>Rename</Button>{device.online && <Button size="sm" onClick={() => void act(device.id, 'disconnect')}>Disconnect</Button>}<Button size="sm" variant="danger" onClick={() => pending === device.id ? void act(device.id, 'revoke').then(() => setPending(null)) : setPending(device.id)}>{pending === device.id ? 'Confirm device removal' : 'Remove device'}</Button></div></article>)}</div>;
}

function pathLabel(session: AgentStatus['sessions'][number]): string {
  if (session.link === 'usb') return 'Câble USB';
  if (session.viaRelay) return 'Relais Internet';
  return /^100\.(6[4-9]|[7-9]\d|1[01]\d|12[0-7])\./.test(session.ip) ? 'Tailscale' : 'Réseau local';
}

function DisplayPage({ status, settings, save }: PageProps) { return <SettingsCard title="Display" subtitle="Écran piloté par l’iPad au début de chaque session (il peut en changer depuis la session)."><Group title="Default display">{status.caps.displays.map((display) => <Row key={display.id} icon="display" title={display.name} subtitle={`${display.width} × ${display.height} · ${display.refreshHz} Hz${display.primary ? ' · Primary' : ''}`} accessory={<input type="radio" name="display" aria-label={`Use ${display.name}`} checked={(settings.display.displayId ?? status.caps.activeDisplayId) === display.id} onChange={() => void save({ ...settings, display: { ...settings.display, displayId: display.id } })} />} />)}</Group><Group title="Power"><Row title="Prevent sleep while connected" subtitle="Keeps the PC available during an active session" accessory={<Switch label="Prevent sleep while connected" checked={settings.display.preventSleep} onChange={(preventSleep) => void save({ ...settings, display: { ...settings.display, preventSleep } })} />} /></Group></SettingsCard>; }

function InputPage({ settings, save }: Omit<PageProps, 'status'>) { const input = settings.input; const patch = (value: Partial<Settings['input']>) => void save({ ...settings, input: { ...input, ...value } }); return <SettingsCard title="Input" subtitle="Every command is validated against a fixed allowlist before Windows receives it."><Group title="Allowed input"><Row icon="keyboard" title="Keyboard & shortcuts" accessory={<Switch label="Keyboard and shortcuts" checked={input.allowKeyboard} onChange={(allowKeyboard) => patch({ allowKeyboard })} />} /><Row icon="pointer" title="Mouse & trackpad" accessory={<Switch label="Mouse and trackpad" checked={input.allowMouse} onChange={(allowMouse) => patch({ allowMouse })} />} /><Row icon="pencil" title="Apple Pencil" accessory={<Switch label="Apple Pencil" checked={input.allowPen} onChange={(allowPen) => patch({ allowPen })} />} /><Row icon="touch" title="Direct touch" accessory={<Switch label="Direct touch" checked={input.allowTouch} onChange={(allowTouch) => patch({ allowTouch })} />} /></Group><Group title="Creative tools"><SettingSegment label="Pencil mode" value={input.penMode} options={[['windows-ink','Windows Ink'],['mouse','Mouse pointer']]} onChange={(penMode) => patch({ penMode: penMode as Settings['input']['penMode'] })} /><Row title="Detect active creative app" subtitle="Selects Blender, Maya or Photoshop controls automatically" accessory={<Switch label="Detect active app" checked={input.detectActiveApp} onChange={(detectActiveApp) => patch({ detectActiveApp })} />} /></Group><Group title="Gestes et pression"><Row icon="pencil" title="Toucher, double tap et sensibilité" subtitle="Dans la session sur l’iPad, ouvrez Commandes puis Apple Pencil et gestes. Les réglages sont propres à chaque tablette." /></Group></SettingsCard>; }

function SecurityPage({ status, settings, save, audit }: PageProps & { audit: AuditEntry[] }) { const security = settings.security; const patch = (value: Partial<Settings['security']>) => void save({ ...settings, security: { ...security, ...value } }); return <><SettingsCard title="Security" subtitle="Zero Trust defaults protect every new device and every incoming command."><Group title="Authorization"><Row title="Confirm every connection" subtitle="Trusted devices still require approval" accessory={<Switch label="Confirm every connection" checked={security.confirmEveryConnection} onChange={(confirmEveryConnection) => patch({ confirmEveryConnection })} />} /><Row title="Notifications" subtitle="Notify when a device connects" accessory={<Switch label="Connection notifications" checked={security.notifyOnConnect} onChange={(notifyOnConnect) => patch({ notifyOnConnect })} />} /><NumberSetting label="Session lifetime" suffix="minutes" value={security.sessionMaxMinutes} min={5} max={10080} onChange={(sessionMaxMinutes) => patch({ sessionMaxMinutes })} /><NumberSetting label="Trust expires after" suffix="days" value={security.trustExpiryDays} min={1} max={3650} onChange={(trustExpiryDays) => patch({ trustExpiryDays })} /></Group><Group title="Brute-force protection"><NumberSetting label="Invalid attempts before lock" suffix="attempts" value={security.lockoutThreshold} min={3} max={20} onChange={(lockoutThreshold) => patch({ lockoutThreshold })} /><NumberSetting label="Lockout duration" suffix="minutes" value={security.lockoutMinutes} min={1} max={1440} onChange={(lockoutMinutes) => patch({ lockoutMinutes })} />{status.lockedSources > 0 && <Row icon="alert" title={`${status.lockedSources} blocked source${status.lockedSources === 1 ? '' : 's'}`} accessory={<Button size="sm" onClick={() => void api.unlock()}>Unlock all</Button>} />}</Group></SettingsCard><section className="desktop-card"><CardTitle title="Security log" subtitle="Append-only local history" /><div className="audit-table">{audit.length ? audit.map((entry, i) => <div className="audit-row" key={`${entry.ts}-${i}`}><Badge tone={entry.severity === 'critical' ? 'red' : entry.severity === 'warning' ? 'orange' : undefined}>{entry.severity}</Badge><span>{entry.message}</span><time>{new Date(entry.ts).toLocaleString()}</time></div>) : <div className="desktop-empty">No events recorded.</div>}</div></section></>; }

function relayBadge(mode: Settings['network']['mode'], configured: boolean, relay: RelayStatus) {
  if (mode !== 'remote-access') return <Badge>Off</Badge>;
  if (!configured) return <Badge tone="orange">Not configured</Badge>;
  if (relay.state === 'connected') return <Badge tone="green" dot>Connected</Badge>;
  if (relay.state === 'error') return <Badge tone="red" dot>Unreachable</Badge>;
  return <Badge tone="orange" dot>Connecting</Badge>;
}

/** What the user must do on the tablet so that its cable carries the session. */
function usbHint(device: UsbTablet): string {
  if (device.kind === 'android') return 'Sur la tablette : Réglages → Connexions → Point d’accès mobile et modem → activez « Modem USB ».';
  if (device.driverMissing) return 'Installez « Apple Devices » depuis le Microsoft Store (pilote USB de l’iPad), puis rebranchez le câble.';
  return 'Sur l’iPad : Réglages → Partage de connexion → activez « Autoriser d’autres utilisateurs », puis touchez « Se fier » si l’iPad le demande (iPad Wi-Fi + Cellular).';
}

function linkState(link: UsbLink): { tone: 'green' | 'orange'; text: string; action?: string } {
  if (link.firewall === 'blocked') return { tone: 'orange', text: `Windows bloque Record OS sur ce réseau ${link.network === 'public' ? '« Public »' : ''}`.trim(), action: 'Autoriser' };
  if (link.internet) return { tone: 'orange', text: 'Internet du PC passe par les données mobiles de la tablette', action: 'Corriger' };
  return { tone: 'green', text: 'Liaison directe par câble' };
}

function UsbCard({ usb, sessions, onAllow }: { usb: UsbStatus; sessions: AgentStatus['sessions']; onAllow: (link: UsbLink) => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const link = usb.links[0];
  if (link) {
    const state = linkState(link);
    const onCable = sessions.find((s) => s.link === 'usb' && s.localIp === link.localIp);
    const blocked = link.firewall === 'blocked';
    const detail = blocked
      ? 'Le pare-feu Windows empêche la tablette de joindre ce PC par le câble. Autorisez la liaison une fois : elle devient un réseau privé réservé à Record OS.'
      : link.internet
        ? 'Windows envoie l’Internet de ce PC par la tablette, au risque de consommer ses données mobiles. Corrigez une fois : Internet reste sur votre Wi-Fi, le flux reste sur le câble.'
        : onCable ? 'Les commandes passent directement par le câble, sans Wi-Fi.' : 'Dès que la tablette se connecte, toute la session passe par le câble. Une session Wi-Fi en cours bascule automatiquement.';
    return <section className="usb-card" data-tone={state.tone}>
      <div className="usb-glyph"><Icon name="cable" size={26} /></div>
      <div className="usb-copy">
        <p className="eyebrow">Câble USB</p>
        <h2>{onCable ? `${onCable.deviceName} connecté par le câble` : `${link.deviceName} relié par câble`}</h2>
        <p>{detail}</p>
        <small className="mono">{link.localIp} · {link.alias}{link.simulated ? ' · simulation' : ''}</small>
      </div>
      {state.action ? <Button variant="primary" icon="shield" loading={busy} onClick={() => { setBusy(true); void onAllow(link).finally(() => setBusy(false)); }}>{state.action}</Button> : <Badge tone="green" dot>{onCable ? 'En direct' : 'Prêt'}</Badge>}
    </section>;
  }
  const device = usb.devices[0]!;
  return <section className="usb-card" data-tone="orange">
    <div className="usb-glyph"><Icon name="cable" size={26} /></div>
    <div className="usb-copy">
      <p className="eyebrow">Câble USB</p>
      <h2>{device.name} branché</h2>
      <p>{usbHint(device)}</p>
    </div>
    <Badge tone="orange" dot>Action requise</Badge>
  </section>;
}

function NetworkPage({status}: PageProps & {onAllowUsb:(link:UsbLink)=>Promise<void>}) { return <SettingsCard title="Connexion Internet" subtitle="Votre PC et votre iPad peuvent utiliser des réseaux différents."><Group title="Record OS"><Row icon="globe" title="Relais sécurisé" subtitle="La connexion est chiffrée de bout en bout." value={status.relay.state === 'connected' ? 'Connecté' : 'Connexion en cours…'} /><Row icon="shield" title="Sans compte" subtitle="Chaque nouvel appareil doit être autorisé sur ce PC." /></Group></SettingsCard>; }

function AdvancedPage({ settings, save }: PageProps) { const advanced = settings.advanced; const patch = (value: Partial<Settings['advanced']>) => void save({ ...settings, advanced: { ...advanced, ...value } }); return <SettingsCard title="Advanced" subtitle="These controls can affect availability and troubleshooting. Secure defaults are recommended."><Group title="Startup"><Row title="Start service with Record OS" accessory={<Switch label="Start service on launch" checked={advanced.startServiceOnLaunch} onChange={(startServiceOnLaunch) => patch({ startServiceOnLaunch })} />} /><Row title="Launch at Windows sign-in" accessory={<Switch label="Launch at sign-in" checked={advanced.launchAtLogin} onChange={(launchAtLogin) => patch({ launchAtLogin })} />} /></Group><Group title="Diagnostics"><Row title="Verbose local logging" accessory={<Switch label="Verbose local logging" checked={advanced.verboseLogging} onChange={(verboseLogging) => patch({ verboseLogging })} />} /><Row title="Developer loopback endpoint" subtitle="127.0.0.1 only; disabled in normal use" accessory={<Switch label="Developer loopback" checked={advanced.developerLoopback} onChange={(developerLoopback) => patch({ developerLoopback })} />} /></Group></SettingsCard>; }

function About({ status }: { status: AgentStatus }) { return <SettingsCard title="Record OS" subtitle="Your computer. Everywhere."><div className="about-mark"><img src={logo} width={64} height={64} alt="" /><h2>Record OS</h2><p>Version {appVersion}</p></div><Group title="This PC"><Row title="Agent ID" value={<span className="mono">{status.agentId}</span>} /><Row title="Identity fingerprint" value={<span className="mono">{status.fingerprint}</span>} /><Row title="Empreinte pour connexion Internet" value={<span className="mono" style={{ overflowWrap: 'anywhere', userSelect: 'all' }}>{status.fullFingerprint ?? 'Mettez à jour Record OS'}</span>} /></Group><p className="about-note">L’iPad sert de tablette graphique et de trackpad : rien n’est capturé ni diffusé depuis ce PC. Les commandes arrivent par une connexion chiffrée de bout en bout, par Internet via le relais Record OS.</p></SettingsCard>; }

type PageProps = { status: AgentStatus; settings: Settings; save: (settings: Settings) => Promise<void> };
function SettingsCard({ title, subtitle, children }: { title: string; subtitle: string; children: React.ReactNode }) { return <section className="settings-layout"><div className="settings-intro"><h2>{title}</h2><p>{subtitle}</p></div><div className="settings-body">{children}</div></section>; }
function SettingSegment({ label, value, options, onChange }: { label: string; value: string; options: [string, string][]; onChange: (value: string) => void }) { return <div className="setting-segment"><span>{label}</span><Segmented value={value} options={options.map(([v, l]) => ({ value: v, label: l }))} onChange={onChange} ariaLabel={label} /></div>; }
function NumberSetting({ label, suffix, value, min, max, onChange }: { label: string; suffix: string; value: number; min: number; max: number; onChange: (value: number) => void }) { return <label className="number-setting"><span>{label}</span><span><input type="number" min={min} max={max} value={value} onChange={(event) => onChange(Number(event.currentTarget.value))} /><small>{suffix}</small></span></label>; }
function Summary({ icon, label, value, detail, onClick }: { icon: IconName; label: string; value: string; detail: string; onClick: () => void }) {
  return <button type="button" className="summary-card" onClick={onClick}>
    <span className="summary-label"><span className="dashboard-icon"><Icon name={icon} size={19} /></span>{label}</span>
    <b>{value}</b><small>{detail}</small>
  </button>;
}
function CardTitle({ title, subtitle, action }: { title: string; subtitle?: string; action?: React.ReactNode }) { return <header className="card-title"><div><h2>{title}</h2>{subtitle && <p>{subtitle}</p>}</div>{action}</header>; }
function Empty({ icon, title, text }: { icon: IconName; title: string; text: string }) { return <div className="desktop-empty large"><Icon name={icon} size={38} /><h2>{title}</h2><p>{text}</p></div>; }

function PairingBanner({ request, onRespond }: { request: AgentStatus['pendingPairing'] & {}; onRespond: (approve: boolean) => Promise<void> }) { return <div className="pairing-banner"><div className="pairing-device"><span><Icon name="tablet" /></span><div><p>Connection request</p><h2>{request.deviceName}</h2><small>{request.deviceKind} · {request.ip} · {request.method}</small></div></div><div className="pairing-code"><small>VERIFY CODE</small><b>{request.sas}</b></div><div className="pairing-actions"><Button variant="danger" onClick={() => void onRespond(false)}>Decline</Button><Button variant="primary" onClick={() => void onRespond(true)}>Allow</Button></div></div>; }
