import { Badge, Button, Group, Icon, IconButton, Row, Segmented, Switch, formatDuration, formatMs, formatRelative, useToast, type IconName } from '@vision-pro/design';
import { useCallback, useEffect, useMemo, useState } from 'react';
import type { RelayStatus, UsbLink, UsbStatus, UsbTablet } from './api.ts';
import { api, type AgentStatus, type AuditEntry, type ConnectAddress, type ConnectInfo, type Device, type Settings } from './api.ts';
import { useDesktopLanguage } from './i18n.ts';

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

  if (!status || !settings) return <div className="desktop-loading"><span className="vp-spinner" /><span>Starting Vision-Pro…</span></div>;

  return (
    <div className="desktop-shell">
      <aside className="sidebar">
        <div className="desktop-wordmark">Vision-Pro</div>
        <nav aria-label="Main navigation">{nav.map((item) => <button key={item.id} type="button" className="nav-item" aria-current={page === item.id ? 'page' : undefined} onClick={() => setPage(item.id)}><Icon name={item.icon} size={19} /><span>{item.label}</span>{item.id === 'devices' && devices.length > 0 && <span className="nav-count">{devices.length}</span>}</button>)}</nav>
        <div className="sidebar-status"><span className="vp-dot" data-tone={status.running ? 'green' : 'red'} /><div><b>{status.running ? 'Ready' : 'Stopped'}</b><small>{status.networkMode === 'local-only' ? 'Local network only' : status.relay.state === 'connected' ? 'Remote access' : 'Relay unreachable'}</small>{status.usb.links.length > 0 && <small className="sidebar-usb"><Icon name="cable" size={12} /> Câble USB actif</small>}</div></div>
      </aside>

      <main className="desktop-main">
        <header className="desktop-header"><div><p className="eyebrow">{status.computerName}</p><h1>{nav.find((item) => item.id === page)?.label}</h1></div><div className="header-actions"><Button onClick={() => void service()} loading={busy}>{status.running ? 'Stop service' : 'Start service'}</Button><Button variant="primary" icon="plus" onClick={() => void openConnect()} disabled={!status.running} loading={busy}>Connect Device</Button></div></header>
        {status.error && <div className="desktop-alert" data-tone="red"><Icon name="alert" /><span>{status.error}</span></div>}
        {status.storeIntegrityWarning && <div className="desktop-alert" data-tone="orange"><Icon name="shield" /><span>The device store failed its integrity check. Every device now requires confirmation.</span></div>}
        {status.pendingPairing && <PairingBanner request={status.pendingPairing} onRespond={async (approve) => { await api.respondPairing(status.pendingPairing!.id, approve); await refresh(); }} />}

        <div className="desktop-content">
          {page === 'home' && <Overview status={status} devices={devices} audit={audit} onConnect={openConnect} onAllowUsb={allowUsb} onEmergency={async () => { const count = await api.emergency(); toast(`${count} connexion${count === 1 ? '' : 's'} arrêtée${count === 1 ? '' : 's'}.`, { icon: 'power', tone: 'orange' }); await refresh(); }} />}
          {page === 'devices' && <Devices devices={devices} act={async (id, action, value) => { await api.deviceAction(id, action, value); await refresh(); }} />}
          {page === 'display' && <DisplayPage status={status} settings={settings} save={save} />}
          {page === 'input' && <InputPage settings={settings} save={save} />}
          {page === 'security' && <SecurityPage status={status} settings={settings} save={save} audit={audit} />}
          {page === 'network' && <NetworkPage status={status} settings={settings} save={save} onAllowUsb={allowUsb} />}
          {page === 'advanced' && <AdvancedPage status={status} settings={settings} save={save} />}
          {page === 'about' && <About status={status} />}
        </div>
      </main>
      {connect && <ConnectDialog info={connect} paired={devices.filter((d) => d.pairedAt >= connectOpenedAt).sort((a, b) => b.pairedAt - a.pairedAt)[0]} onRefresh={async () => setConnect(await api.openConnect(true))} onClose={() => { setConnect(null); void api.closeConnect(); }} />}
    </div>
  );
}

function Overview({ status, devices, audit, onConnect, onEmergency, onAllowUsb }: { status: AgentStatus; devices: Device[]; audit: AuditEntry[]; onConnect: () => void; onEmergency: () => void; onAllowUsb: (link: UsbLink) => Promise<void> }) {
  const session = status.sessions[0];
  return <>
    <section className="status-hero" data-connected={session ? 'true' : undefined}><div className="status-orb"><span className="vp-dot" data-tone={status.running ? 'green' : 'red'} /></div><div className="status-copy"><p className="eyebrow">Status</p><h2>{session ? 'Connected' : status.running ? 'Ready' : 'Service stopped'}</h2><p>{session ? (session.permission === 'control' ? `${session.deviceName} pilote ce PC.` : `${session.deviceName} est connecté, commandes désactivées.`) : status.running ? 'Vision-Pro attend de manière sécurisée sur votre réseau privé.' : 'Démarrez le service pour rendre ce PC disponible.'}</p></div>{session ? <Button variant="danger" icon="power" onClick={onEmergency}>Disconnect</Button> : <Button variant="primary" icon="plus" onClick={onConnect} disabled={!status.running}>Connect Device</Button>}</section>
    <div className="summary-grid"><Summary icon={session?.link === 'usb' ? 'cable' : 'tablet'} label="Connected device" value={session?.deviceName ?? 'None'} detail={session ? `${session.ip}${session.link === 'usb' ? ' · câble USB' : session.viaRelay ? ' · via relay' : ''}` : `${devices.filter((d) => d.status === 'trusted').length} trusted`} /><Summary icon="display" label="Display" value={(status.caps.displays.find((d) => d.id === status.caps.activeDisplayId) ?? status.caps.displays[0])?.name ?? 'Unavailable'} detail={status.caps.displays.length > 1 ? `${status.caps.displays.length} écrans` : 'Écran piloté'} /><Summary icon="gauge" label="Latency" value={session?.rttMs != null ? formatMs(session.rttMs / 2) : '—'} detail={session ? `${Math.round(session.inputRate)} commandes/s` : 'Aucune session'} /><Summary icon="shield" label="Security" value="Protected" detail={status.networkMode === 'local-only' ? 'Local Only' : 'Remote Access'} /></div>
    {status.usb.enabled && (status.usb.links.length > 0 || status.usb.devices.length > 0) && <UsbCard usb={status.usb} sessions={status.sessions} onAllow={onAllowUsb} />}
    {session && <section className="desktop-card"><CardTitle title="Session en cours" subtitle={session.activeApp ?? 'Bureau Windows'} action={<Badge tone="green" dot>{session.link === 'usb' ? 'Câble USB' : session.viaRelay ? 'Relais' : 'Connecté'}</Badge>} /><div className="live-metrics"><Metric value={session.rttMs != null ? formatMs(session.rttMs / 2) : '—'} label="Latence des commandes" /><Metric value={`${Math.round(session.inputRate)}/s`} label="Commandes reçues" /><Metric value={pathLabel(session)} label="Chemin" /><Metric value={session.permission === 'control' ? 'Activées' : 'Désactivées'} label="Commandes" /></div></section>}
    <section className="desktop-card"><CardTitle title="Recent security activity" subtitle="Security-relevant events from this PC" />{audit.length ? <div className="activity-list">{audit.slice(0, 5).map((entry, i) => <div key={`${entry.ts}-${i}`} className="activity"><span className="vp-dot" data-tone={entry.severity === 'critical' ? 'red' : entry.severity === 'warning' ? 'orange' : 'green'} /><div><b>{entry.message}</b><small>{formatRelative(entry.ts)}</small></div></div>)}</div> : <div className="desktop-empty">No security events yet.</div>}</section>
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

function InputPage({ settings, save }: Omit<PageProps, 'status'>) { const input = settings.input; const patch = (value: Partial<Settings['input']>) => void save({ ...settings, input: { ...input, ...value } }); return <SettingsCard title="Input" subtitle="Every command is validated against a fixed allowlist before Windows receives it."><Group title="Allowed input"><Row icon="keyboard" title="Keyboard & shortcuts" accessory={<Switch label="Keyboard and shortcuts" checked={input.allowKeyboard} onChange={(allowKeyboard) => patch({ allowKeyboard })} />} /><Row icon="pointer" title="Mouse & trackpad" accessory={<Switch label="Mouse and trackpad" checked={input.allowMouse} onChange={(allowMouse) => patch({ allowMouse })} />} /><Row icon="pencil" title="Apple Pencil" accessory={<Switch label="Apple Pencil" checked={input.allowPen} onChange={(allowPen) => patch({ allowPen })} />} /><Row icon="touch" title="Direct touch" accessory={<Switch label="Direct touch" checked={input.allowTouch} onChange={(allowTouch) => patch({ allowTouch })} />} /></Group><Group title="Creative tools"><SettingSegment label="Pencil mode" value={input.penMode} options={[['windows-ink','Windows Ink'],['mouse','Mouse pointer']]} onChange={(penMode) => patch({ penMode: penMode as Settings['input']['penMode'] })} /><Row title="Detect active creative app" subtitle="Selects Blender, Maya or Photoshop controls automatically" accessory={<Switch label="Detect active app" checked={input.detectActiveApp} onChange={(detectActiveApp) => patch({ detectActiveApp })} />} /></Group></SettingsCard>; }

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
  if (link.firewall === 'blocked') return { tone: 'orange', text: `Windows bloque Vision-Pro sur ce réseau ${link.network === 'public' ? '« Public »' : ''}`.trim(), action: 'Autoriser' };
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
      ? 'Le pare-feu Windows empêche la tablette de joindre ce PC par le câble. Autorisez la liaison une fois : elle devient un réseau privé réservé à Vision-Pro.'
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

function NetworkPage({ status, settings, save, onAllowUsb }: PageProps & { onAllowUsb: (link: UsbLink) => Promise<void> }) {
  const network = settings.network;
  const toast = useToast();
  const [relayUrl, setRelayUrl] = useState(network.relayUrl ?? '');
  const [relayToken, setRelayToken] = useState(network.relayToken ?? '');
  useEffect(() => { setRelayUrl(network.relayUrl ?? ''); setRelayToken(network.relayToken ?? ''); }, [network.relayUrl, network.relayToken]);
  const patch = (value: Partial<Settings['network']>) => void save({ ...settings, network: { ...network, ...value } });
  const relay = status.relay;
  const remote = network.mode === 'remote-access';
  const dirty = relayUrl.trim() !== (network.relayUrl ?? '') || relayToken.trim() !== (network.relayToken ?? '');
  const saveRelay = () => {
    const url = relayUrl.trim();
    const token = relayToken.trim();
    if (url && !/^((wss|https):\/\/)?[a-z0-9.-]+(:\d{1,5})?\/?$/i.test(url) && !/^(ws|http):\/\/(127\.0\.0\.1|localhost)(:\d{1,5})?\/?$/i.test(url)) {
      toast('Enter the relay address, for example relay.example.com.', { icon: 'alert', tone: 'red' });
      return;
    }
    if (token && token.length < 16) {
      toast('The access token must contain at least 16 characters.', { icon: 'alert', tone: 'red' });
      return;
    }
    patch({ relayUrl: url || null, relayToken: token || null });
  };
  const relayTitle = relay.state === 'connected' ? 'Reachable from the Internet' : relay.state === 'error' ? 'Relay unreachable' : relay.state === 'connecting' ? 'Connecting to the relay…' : 'Relay inactive';
  const relayDetail = relay.state === 'connected' ? 'Session chiffrée de bout en bout' : relay.error ?? (network.relayUrl ? 'Save to connect' : 'Enter your relay address');
  const tailscale = status.addresses.find((address) => /^https:\/\/100\.(6[4-9]|[7-9]\d|1[01]\d|12[0-7])\./.test(address));
  return <SettingsCard title="Network" subtitle="Local Only never exposes the PC to the Internet. Remote Access goes through your own relay, with end-to-end encryption.">
    <div className="network-modes">
      <button type="button" aria-pressed={network.mode === 'local-only'} onClick={() => patch({ mode: 'local-only' })}><Icon name="wifi" /><b>Local Only</b><span>Direct iPad ↔ PC connection on your private network.</span><Badge tone="green">Recommended</Badge></button>
      <button type="button" aria-pressed={remote} onClick={() => patch({ mode: 'remote-access' })}><Icon name="globe" /><b>Remote Access</b><span>Joignez ce PC depuis n’importe quel réseau via votre relais Vision-Pro. Le relais ne voit jamais vos commandes.</span>{relayBadge(network.mode, !!network.relayUrl, relay)}</button>
    </div>
    {remote && <Group title="Relay">
      <label className="text-setting"><span>Relay address</span><input className="vp-input" placeholder="relay.example.com" value={relayUrl} autoCapitalize="off" autoCorrect="off" spellCheck={false} onChange={(event) => setRelayUrl(event.currentTarget.value)} onKeyDown={(event) => { if (event.key === 'Enter' && dirty) saveRelay(); }} /></label>
      <label className="text-setting"><span>Access token</span><input className="vp-input" type="password" placeholder="Optional" value={relayToken} autoComplete="off" onChange={(event) => setRelayToken(event.currentTarget.value)} onKeyDown={(event) => { if (event.key === 'Enter' && dirty) saveRelay(); }} /></label>
      <Row icon="globe" title={relayTitle} subtitle={relayDetail} accessory={<Button size="sm" variant={dirty ? 'primary' : 'default'} disabled={!dirty} onClick={saveRelay}>Save</Button>} />
    </Group>}
    {remote && <p className="about-note">Associez chaque iPad une fois sur le réseau local. Il se connecte ensuite depuis n’importe où : la clé du PC est épinglée, un code est toujours exigé pour associer via le relais, et les commandes restent chiffrées de bout en bout.</p>}
    <Group title="Hors de chez vous (Tailscale)">
      {tailscale
        ? <Row icon="globe" title="Tailscale actif sur ce PC" subtitle={`Depuis n’importe quel réseau, l’iPad (connecté à Tailscale) ouvre ${tailscale}`} accessory={<Badge tone="green" dot>Prêt</Badge>} />
        : <Row icon="globe" title="Tailscale n’est pas détecté" subtitle="Installez Tailscale sur ce PC et sur l’iPad avec le même compte : l’iPad joint alors ce PC depuis n’importe où, sans relais ni ouverture de port." accessory={<Badge>Facultatif</Badge>} />}
    </Group>
    <Group title="Câble USB">
      <Row icon="cable" title="Connexion par câble USB" subtitle="Détecte la tablette branchée et fait passer toute la session par le câble" accessory={<Switch label="Connexion par câble USB" checked={network.usbDirect} onChange={(usbDirect) => patch({ usbDirect })} />} />
      {network.usbDirect && status.usb.links.map((link) => <Row key={link.id} icon="tablet" title={link.deviceName} subtitle={`${linkState(link).text} · ${link.localIp} · ${link.alias}`} accessory={linkState(link).action ? <Button size="sm" variant="primary" onClick={() => void onAllowUsb(link)}>{linkState(link).action}</Button> : <Badge tone="green" dot>Actif</Badge>} />)}
      {network.usbDirect && status.usb.devices.filter((d) => !status.usb.links.some((l) => l.kind === d.kind)).map((device) => <Row key={device.id} icon="tablet" title={`${device.name} branché`} subtitle={usbHint(device)} accessory={<Badge tone="orange" dot>En attente</Badge>} />)}
      {network.usbDirect && !status.usb.links.length && !status.usb.devices.length && <Row title="Aucune tablette branchée" subtitle="Branchez l’iPad en USB-C ou avec un câble Lightning ; il est détecté instantanément." />}
    </Group>
    {network.usbDirect && <p className="about-note">Le câble transporte la connexion chiffrée et les commandes : aucune box ni Wi-Fi n’est nécessaire. L’iPad crée la liaison réseau par le câble grâce au Partage de connexion ; si le câble est débranché, la session repasse aussitôt par le Wi-Fi.</p>}
    <Group title="Discovery"><Row title="Advertise this PC nearby" accessory={<Switch label="Advertise this PC nearby" checked={network.advertiseMdns} onChange={(advertiseMdns) => patch({ advertiseMdns })} />} /><NumberSetting label="Secure local port" suffix="TCP" value={network.httpsPort} min={1024} max={65535} onChange={(httpsPort) => patch({ httpsPort })} /></Group></SettingsCard>;
}

function AdvancedPage({ settings, save }: PageProps) { const advanced = settings.advanced; const patch = (value: Partial<Settings['advanced']>) => void save({ ...settings, advanced: { ...advanced, ...value } }); return <SettingsCard title="Advanced" subtitle="These controls can affect availability and troubleshooting. Secure defaults are recommended."><Group title="Startup"><Row title="Start service with Vision-Pro" accessory={<Switch label="Start service on launch" checked={advanced.startServiceOnLaunch} onChange={(startServiceOnLaunch) => patch({ startServiceOnLaunch })} />} /><Row title="Launch at Windows sign-in" accessory={<Switch label="Launch at sign-in" checked={advanced.launchAtLogin} onChange={(launchAtLogin) => patch({ launchAtLogin })} />} /></Group><Group title="Diagnostics"><Row title="Verbose local logging" accessory={<Switch label="Verbose local logging" checked={advanced.verboseLogging} onChange={(verboseLogging) => patch({ verboseLogging })} />} /><Row title="Developer loopback endpoint" subtitle="127.0.0.1 only; disabled in normal use" accessory={<Switch label="Developer loopback" checked={advanced.developerLoopback} onChange={(developerLoopback) => patch({ developerLoopback })} />} /></Group></SettingsCard>; }

function About({ status }: { status: AgentStatus }) { return <SettingsCard title="Vision-Pro" subtitle="Your computer. Everywhere."><div className="about-mark"><Icon name="devices" size={44} /><h2>Vision-Pro</h2><p>Development code name · Version {status.version}</p></div><Group title="This PC"><Row title="Agent ID" value={<span className="mono">{status.agentId}</span>} /><Row title="Identity fingerprint" value={<span className="mono">{status.fingerprint}</span>} /><Row title="Empreinte pour connexion Internet" value={<span className="mono" style={{ overflowWrap: 'anywhere', userSelect: 'all' }}>{status.fullFingerprint ?? 'Mettez à jour Vision-Pro'}</span>} /><Row title="Local hostname" value={status.hostname} /></Group><p className="about-note">L’iPad sert de tablette graphique et de trackpad : rien n’est capturé ni diffusé depuis ce PC. Les commandes arrivent par une connexion chiffrée de bout en bout, directement sur le réseau local, par Tailscale ou par câble.</p></SettingsCard>; }

type PageProps = { status: AgentStatus; settings: Settings; save: (settings: Settings) => Promise<void> };
function SettingsCard({ title, subtitle, children }: { title: string; subtitle: string; children: React.ReactNode }) { return <section className="settings-layout"><div className="settings-intro"><h2>{title}</h2><p>{subtitle}</p></div><div className="settings-body">{children}</div></section>; }
function SettingSegment({ label, value, options, onChange }: { label: string; value: string; options: [string, string][]; onChange: (value: string) => void }) { return <div className="setting-segment"><span>{label}</span><Segmented value={value} options={options.map(([v, l]) => ({ value: v, label: l }))} onChange={onChange} ariaLabel={label} /></div>; }
function NumberSetting({ label, suffix, value, min, max, onChange }: { label: string; suffix: string; value: number; min: number; max: number; onChange: (value: number) => void }) { return <label className="number-setting"><span>{label}</span><span><input type="number" min={min} max={max} value={value} onChange={(event) => onChange(Number(event.currentTarget.value))} /><small>{suffix}</small></span></label>; }
function Summary({ icon, label, value, detail }: { icon: IconName; label: string; value: string; detail: string }) { return <article className="summary-card"><Icon name={icon} /><span>{label}</span><b>{value}</b><small>{detail}</small></article>; }
function Metric({ value, label }: { value: string; label: string }) { return <div className="live-metric"><b>{value}</b><span>{label}</span></div>; }
function CardTitle({ title, subtitle, action }: { title: string; subtitle: string; action?: React.ReactNode }) { return <header className="card-title"><div><h2>{title}</h2><p>{subtitle}</p></div>{action}</header>; }
function Empty({ icon, title, text }: { icon: IconName; title: string; text: string }) { return <div className="desktop-empty large"><Icon name={icon} size={38} /><h2>{title}</h2><p>{text}</p></div>; }

function PairingBanner({ request, onRespond }: { request: AgentStatus['pendingPairing'] & {}; onRespond: (approve: boolean) => Promise<void> }) { return <div className="pairing-banner"><div className="pairing-device"><span><Icon name="tablet" /></span><div><p>Connection request</p><h2>{request.deviceName}</h2><small>{request.deviceKind} · {request.ip} · {request.method}</small></div></div><div className="pairing-code"><small>VERIFY CODE</small><b>{request.sas}</b></div><div className="pairing-actions"><Button variant="danger" onClick={() => void onRespond(false)}>Decline</Button><Button variant="primary" onClick={() => void onRespond(true)}>Allow</Button></div></div>; }

const networkLabel: Record<ConnectAddress['network'], { title: string; hint: string }> = {
  lan: { title: 'Sur le même Wi-Fi', hint: 'iPad et PC sur le même réseau' },
  tailscale: { title: 'Depuis n’importe où (Tailscale)', hint: 'iPad connecté à Tailscale' },
};

function ConnectDialog({ info, paired, onRefresh, onClose }: { info: ConnectInfo; paired?: Device; onRefresh: () => Promise<void>; onClose: () => void }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => { const timer = window.setInterval(() => setNow(Date.now()), 1000); return () => window.clearInterval(timer); }, []);
  const remaining = info.expiresAt - now;
  const code = `${info.code.slice(0, 3)} ${info.code.slice(3)}`;
  return <div className="dialog-scrim"><section className="connect-dialog" role="dialog" aria-modal="true" aria-labelledby="connect-title">
    <div className="connect-head"><div><p className="eyebrow">Association sécurisée</p><h2 id="connect-title">Connecter un iPad</h2></div><IconButton icon="close" label="Close" onClick={onClose} /></div>
    {paired && <div className="connect-success"><Icon name="check" /><span><b>{paired.name}</b> est associé. Vous pouvez fermer cette fenêtre, ou associer un autre iPad avec le nouveau code.</span></div>}
    <ol className="connect-steps">
      <li><span className="step-n">1</span><div><b>Sur l’iPad, ouvrez Safari et tapez cette adresse</b>
        {info.addresses.length ? <div className="connect-addresses">{info.addresses.map((address) => <div key={address.url} className="connect-address"><small>{networkLabel[address.network].title}</small><code>{address.url}</code><span>{networkLabel[address.network].hint}</span></div>)}</div>
          : <p className="connect-warning">Aucune adresse réseau : connectez ce PC au Wi-Fi (ou à Tailscale).</p>}
        <small className="connect-note">La première fois, la page fait installer le profil de sécurité de ce PC, puis ouvre Vision-Pro toute seule.</small></div></li>
      <li><span className="step-n">2</span><div><b>Entrez ce code</b><div className="pair-code" aria-label={`Code ${info.code}`}><b>{code}</b><span>{remaining > 0 ? `expire dans ${formatDuration(remaining)}` : 'code expiré'}</span></div></div></li>
    </ol>
    <div className="connect-actions"><Button icon="refresh" onClick={() => void onRefresh()}>Nouveau code</Button></div>
    <div className="trust-note"><Icon name="shield" /><span><b>Le code est l’autorisation.</b> Seul un iPad qui le saisit est associé ; un code ne sert qu’une fois et en remplace un autre après cinq erreurs.</span></div>
  </section></div>;
}
