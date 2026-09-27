import { Badge, Button, Group, Icon, IconButton, Row, Segmented, Switch, formatBitrate, formatDuration, formatMs, formatRelative, useToast, type IconName } from '@vision-pro/design';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { api, type AgentStatus, type AuditEntry, type ConnectInfo, type Device, type PermissionKind, type Permissions, type Settings } from './api.ts';
import { useDesktopLanguage } from './i18n.ts';

type Page = 'home' | 'devices' | 'display' | 'streaming' | 'input' | 'security' | 'network' | 'advanced' | 'about';
const nav: { id: Page; label: string; icon: IconName }[] = [
  { id: 'home', label: 'Overview', icon: 'home' }, { id: 'devices', label: 'Devices', icon: 'devices' }, { id: 'display', label: 'Display', icon: 'display' },
  { id: 'streaming', label: 'Streaming', icon: 'gauge' }, { id: 'input', label: 'Input', icon: 'pointer' }, { id: 'security', label: 'Security', icon: 'shield' },
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
  const [permissions, setPermissions] = useState<Permissions | null>(null);
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const refresh = useCallback(async () => {
    const [nextStatus, nextSettings, nextDevices, nextAudit, nextPermissions] = await Promise.all([api.status(), api.settings(), api.devices(), api.audit(100), api.permissions()]);
    setStatus(nextStatus); setSettings(nextSettings); setDevices(nextDevices); setAudit(nextAudit); setPermissions(nextPermissions);
  }, []);

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
    try { setConnect(await api.openConnect(false)); }
    catch (error) { toast(String(error), { icon: 'alert', tone: 'red' }); }
    finally { setBusy(false); }
  };

  if (!status || !settings) return <div className="desktop-loading"><span className="vp-spinner" /><span>Starting Vision-Pro…</span></div>;

  return (
    <div className="desktop-shell">
      <aside className="sidebar">
        <div className="sidebar-titlebar" data-tauri-drag-region />
        <div className="desktop-wordmark" data-tauri-drag-region>Vision-Pro</div>
        <nav aria-label="Main navigation">{nav.map((item) => <button key={item.id} type="button" className="nav-item" aria-current={page === item.id ? 'page' : undefined} onClick={() => setPage(item.id)}><Icon name={item.icon} size={19} /><span>{item.label}</span>{item.id === 'devices' && devices.length > 0 && <span className="nav-count">{devices.length}</span>}</button>)}</nav>
        <div className="sidebar-status"><span className="vp-dot" data-tone={status.running ? 'green' : 'red'} /><div><b>{status.running ? 'Ready' : 'Stopped'}</b><small>{status.networkMode === 'local-only' ? 'Local network only' : 'Remote access'}</small></div></div>
      </aside>

      <main className="desktop-main">
        <header className="desktop-header" data-tauri-drag-region><div data-tauri-drag-region><p className="eyebrow">{status.computerName}</p><h1>{nav.find((item) => item.id === page)?.label}</h1></div><div className="header-actions"><Button onClick={() => void service()} loading={busy}>{status.running ? 'Stop service' : 'Start service'}</Button><Button variant="primary" icon="plus" onClick={() => void openConnect()} disabled={!status.running} loading={busy}>Connect Device</Button></div></header>
        {status.error && <div className="desktop-alert" data-tone="red"><Icon name="alert" /><span>{status.error}</span></div>}
        {status.storeIntegrityWarning && <div className="desktop-alert" data-tone="orange"><Icon name="shield" /><span>The device store failed its integrity check. Every device now requires confirmation.</span></div>}
        {permissions && (!permissions.screenRecording || !permissions.accessibility) && page !== 'security' && <PermissionsBanner permissions={permissions} onOpen={() => setPage('security')} />}
        {status.pendingPairing && <PairingBanner request={status.pendingPairing} onRespond={async (approve) => { await api.respondPairing(status.pendingPairing!.id, approve); await refresh(); }} />}

        <div className="desktop-content">
          {page === 'home' && <Overview status={status} devices={devices} audit={audit} onConnect={openConnect} onEmergency={async () => { const count = await api.emergency(); toast(`${count} connexion${count === 1 ? '' : 's'} arrêtée${count === 1 ? '' : 's'}.`, { icon: 'power', tone: 'orange' }); await refresh(); }} />}
          {page === 'devices' && <Devices devices={devices} act={async (id, action, value) => { await api.deviceAction(id, action, value); await refresh(); }} />}
          {page === 'display' && <DisplayPage status={status} settings={settings} save={save} />}
          {page === 'streaming' && <StreamingPage status={status} settings={settings} save={save} />}
          {page === 'input' && <InputPage settings={settings} save={save} />}
          {page === 'security' && <SecurityPage status={status} settings={settings} save={save} audit={audit} permissions={permissions} onPermission={async (kind) => { await api.requestPermission(kind); await refresh(); }} />}
          {page === 'network' && <NetworkPage settings={settings} save={save} />}
          {page === 'advanced' && <AdvancedPage status={status} settings={settings} save={save} />}
          {page === 'about' && <About status={status} />}
        </div>
      </main>
      {connect && <ConnectDialog info={connect} onRefresh={async () => setConnect(await api.openConnect(true))} onClose={() => { setConnect(null); void api.closeConnect(); }} />}
    </div>
  );
}

function Overview({ status, devices, audit, onConnect, onEmergency }: { status: AgentStatus; devices: Device[]; audit: AuditEntry[]; onConnect: () => void; onEmergency: () => void }) {
  const session = status.sessions[0];
  return <>
    <section className="status-hero" data-connected={session ? 'true' : undefined}><div className="status-orb"><span className="vp-dot" data-tone={status.running ? 'green' : 'red'} /></div><div className="status-copy"><p className="eyebrow">Status</p><h2>{session ? 'Connected' : status.running ? 'Ready' : 'Service stopped'}</h2><p>{session ? `${session.deviceName} is ${session.permission === 'control' ? 'controlling' : 'viewing'} this Mac.` : status.running ? 'Vision-Pro is waiting securely on your private network.' : 'Start the service to make this Mac available.'}</p></div>{session ? <Button variant="danger" icon="power" onClick={onEmergency}>Disconnect</Button> : <Button variant="primary" icon="plus" onClick={onConnect} disabled={!status.running}>Connect Device</Button>}</section>
    <div className="summary-grid"><Summary icon="tablet" label="Connected device" value={session?.deviceName ?? 'None'} detail={session?.ip ?? `${devices.filter((d) => d.status === 'trusted').length} trusted`} /><Summary icon="display" label="Display" value={session?.video ? `${session.video.width} × ${session.video.height}` : status.caps.displays[0]?.name ?? 'Unavailable'} detail={session?.video ? `${session.video.fps} FPS` : `${status.caps.displays[0]?.refreshHz ?? 0} Hz`} /><Summary icon="gauge" label="Latency" value={session?.rttMs != null ? formatMs(session.rttMs) : '—'} detail={session?.stats ? `${session.stats.sentFps.toFixed(0)} FPS sent` : 'No active stream'} /><Summary icon="shield" label="Security" value="Protected" detail={status.networkMode === 'local-only' ? 'Local Only' : 'Remote Access'} /></div>
    {session && <section className="desktop-card"><CardTitle title="Live streaming" subtitle={session.activeApp ?? 'macOS desktop'} action={<Badge tone="green" dot>Connected</Badge>} /><div className="live-metrics"><Metric value={session.video?.codec.toUpperCase() ?? '—'} label="Codec" /><Metric value={session.video?.hardware ? 'Hardware' : 'Software'} label="Encoder" /><Metric value={formatBitrate(session.stats?.sentKbps)} label="Bitrate" /><Metric value={formatMs(session.stats?.inputProcessMs)} label="Input processing" /></div></section>}
    <section className="desktop-card"><CardTitle title="Recent security activity" subtitle="Security-relevant events from this Mac" />{audit.length ? <div className="activity-list">{audit.slice(0, 5).map((entry, i) => <div key={`${entry.ts}-${i}`} className="activity"><span className="vp-dot" data-tone={entry.severity === 'critical' ? 'red' : entry.severity === 'warning' ? 'orange' : 'green'} /><div><b>{entry.message}</b><small>{formatRelative(entry.ts)}</small></div></div>)}</div> : <div className="desktop-empty">No security events yet.</div>}</section>
  </>;
}

function Devices({ devices, act }: { devices: Device[]; act: (id: string, action: string, value?: string) => Promise<void> }) {
  const [pending, setPending] = useState<string | null>(null);
  if (!devices.length) return <Empty icon="devices" title="No paired devices" text="Choose Connect Device to pair an iPad. Discovery never grants access automatically." />;
  const statusText = { trusted: 'De confiance', untrusted: 'À confirmer', revoked: 'Révoqué' } as const;
  return <div className="device-grid">{devices.map((device) => <article key={device.id} className="device-card"><div className="device-top"><span className="device-icon"><Icon name="tablet" /></span><div><h3>{device.name}</h3><p>{device.kind} · {device.lastSeenAt ? `Last connected ${formatRelative(device.lastSeenAt)}` : 'Never connected'}</p></div><Badge tone={device.online ? 'green' : device.status === 'untrusted' ? 'orange' : undefined} dot>{device.online ? 'Online' : statusText[device.status]}</Badge></div><dl className="device-details"><dt>Permission</dt><dd>{device.permission === 'control' ? 'View & Control' : 'View Only'}</dd><dt>Last address</dt><dd>{device.lastIp ?? '—'}</dd><dt>Key</dt><dd className="mono">{device.fingerprintDisplay || 'Unavailable'}</dd></dl><div className="device-actions">{device.status === 'untrusted' && <Button size="sm" onClick={() => void act(device.id, 'trust')}>Trust Device</Button>}<Button size="sm" onClick={() => { const name = prompt('Device name', device.name); if (name) void act(device.id, 'rename', name); }}>Rename</Button>{device.online && <Button size="sm" onClick={() => void act(device.id, 'disconnect')}>Disconnect</Button>}<Button size="sm" variant="danger" onClick={() => pending === device.id ? void act(device.id, 'revoke').then(() => setPending(null)) : setPending(device.id)}>{pending === device.id ? 'Confirm device removal' : 'Remove device'}</Button></div></article>)}</div>;
}

function DisplayPage({ status, settings, save }: PageProps) { return <SettingsCard title="Display" subtitle="Choose what Vision-Pro streams by default."><Group title="Default display">{status.caps.displays.map((display) => <Row key={display.id} icon="display" title={display.name} subtitle={`${display.width} × ${display.height} · ${display.refreshHz} Hz${display.primary ? ' · Primary' : ''}`} accessory={<input type="radio" name="display" aria-label={`Use ${display.name}`} checked={(settings.display.displayId ?? status.caps.activeDisplayId) === display.id} onChange={() => void save({ ...settings, display: { ...settings.display, displayId: display.id } })} />} />)}</Group><Group title="Power"><Row title="Prevent sleep while connected" subtitle="Keeps the Mac and its display awake during an active session" accessory={<Switch label="Prevent sleep while connected" checked={settings.display.preventSleep} onChange={(preventSleep) => void save({ ...settings, display: { ...settings.display, preventSleep } })} />} /></Group></SettingsCard>; }

function StreamingPage({ status, settings, save }: PageProps) { const s = settings.streaming; const patch = (value: Partial<Settings['streaming']>) => void save({ ...settings, streaming: { ...s, ...value } }); return <SettingsCard title="Streaming" subtitle="Vision-Pro uses a direct local WebRTC path and the Mac’s hardware media engine (VideoToolbox), applying the best format supported by both the Mac and the iPad."><div className="encoder-strip">{status.caps.encoders.map((encoder) => <Badge key={`${encoder.codec}-${encoder.name}`} tone={encoder.hardware ? 'green' : undefined} dot>{encoder.name}</Badge>)}</div><Group title="Performance"><SettingSegment label="Resolution" value={s.video.resolution} options={[['720p','720p'],['1080p','1080p'],['1440p','2K'],['2160p','4K'],['4320p','8K'],['native','Native']]} onChange={(resolution) => patch({ video: { ...s.video, resolution } })} /><SettingSegment label="Encoder" value={s.encoder} options={[['auto','Auto'],['hardware-only','Hardware only'],['software','Software']]} onChange={(encoder) => patch({ encoder: encoder as Settings['streaming']['encoder'] })} /><SettingSegment label="Profile" value={s.video.profile} options={[['balanced','Balanced'],['low-latency','Low Latency'],['max-quality','Maximum Quality'],['battery-saver','Battery Saver']]} onChange={(profile) => patch({ video: { ...s.video, profile } })} /><SettingSegment label="Codec" value={s.video.codec} options={[['auto','Auto'],['h264','H.264'],['hevc','HEVC']]} onChange={(codec) => patch({ video: { ...s.video, codec } })} /><Row title="Allow iPad quality changes" accessory={<Switch label="Allow iPad quality changes" checked={s.allowClientOverrides} onChange={(allowClientOverrides) => patch({ allowClientOverrides })} />} /></Group></SettingsCard>; }

function InputPage({ settings, save }: Omit<PageProps, 'status'>) { const input = settings.input; const patch = (value: Partial<Settings['input']>) => void save({ ...settings, input: { ...input, ...value } }); return <SettingsCard title="Input" subtitle="Every command is validated against a fixed allowlist before macOS receives it."><Group title="Allowed input"><Row icon="keyboard" title="Keyboard & shortcuts" accessory={<Switch label="Keyboard and shortcuts" checked={input.allowKeyboard} onChange={(allowKeyboard) => patch({ allowKeyboard })} />} /><Row icon="pointer" title="Mouse & trackpad" accessory={<Switch label="Mouse and trackpad" checked={input.allowMouse} onChange={(allowMouse) => patch({ allowMouse })} />} /><Row icon="pencil" title="Apple Pencil" accessory={<Switch label="Apple Pencil" checked={input.allowPen} onChange={(allowPen) => patch({ allowPen })} />} /><Row icon="touch" title="Direct touch" accessory={<Switch label="Direct touch" checked={input.allowTouch} onChange={(allowTouch) => patch({ allowTouch })} />} /></Group><Group title="Creative tools"><SettingSegment label="Pencil mode" value={input.penMode} options={[['tablet','Tablet pen'],['mouse','Mouse pointer']]} onChange={(penMode) => patch({ penMode: penMode as Settings['input']['penMode'] })} /><Row title="Detect active creative app" subtitle="Selects Blender, Maya or Photoshop controls automatically" accessory={<Switch label="Detect active app" checked={input.detectActiveApp} onChange={(detectActiveApp) => patch({ detectActiveApp })} />} /></Group></SettingsCard>; }

function SecurityPage({ status, settings, save, audit, permissions, onPermission }: PageProps & { audit: AuditEntry[]; permissions: Permissions | null; onPermission: (kind: PermissionKind) => Promise<void> }) { const security = settings.security; const patch = (value: Partial<Settings['security']>) => void save({ ...settings, security: { ...security, ...value } }); return <><SettingsCard title="macOS permissions" subtitle="macOS asks you to allow screen recording and input control once. Vision-Pro cannot grant them to itself."><PermissionsGroup permissions={permissions} onPermission={onPermission} /></SettingsCard><SettingsCard title="Security" subtitle="Zero Trust defaults protect every new device and every incoming command."><Group title="Authorization"><Row title="Confirm every connection" subtitle="Trusted devices still require approval" accessory={<Switch label="Confirm every connection" checked={security.confirmEveryConnection} onChange={(confirmEveryConnection) => patch({ confirmEveryConnection })} />} /><Row title="Notifications" subtitle="Notify when a device connects" accessory={<Switch label="Connection notifications" checked={security.notifyOnConnect} onChange={(notifyOnConnect) => patch({ notifyOnConnect })} />} /><NumberSetting label="Session lifetime" suffix="minutes" value={security.sessionMaxMinutes} min={5} max={10080} onChange={(sessionMaxMinutes) => patch({ sessionMaxMinutes })} /><NumberSetting label="Trust expires after" suffix="days" value={security.trustExpiryDays} min={1} max={3650} onChange={(trustExpiryDays) => patch({ trustExpiryDays })} /></Group><Group title="Brute-force protection"><NumberSetting label="Invalid attempts before lock" suffix="attempts" value={security.lockoutThreshold} min={3} max={20} onChange={(lockoutThreshold) => patch({ lockoutThreshold })} /><NumberSetting label="Lockout duration" suffix="minutes" value={security.lockoutMinutes} min={1} max={1440} onChange={(lockoutMinutes) => patch({ lockoutMinutes })} />{status.lockedSources > 0 && <Row icon="alert" title={`${status.lockedSources} blocked source${status.lockedSources === 1 ? '' : 's'}`} accessory={<Button size="sm" onClick={() => void api.unlock()}>Unlock all</Button>} />}</Group></SettingsCard><section className="desktop-card"><CardTitle title="Security log" subtitle="Append-only local history" /><div className="audit-table">{audit.length ? audit.map((entry, i) => <div className="audit-row" key={`${entry.ts}-${i}`}><Badge tone={entry.severity === 'critical' ? 'red' : entry.severity === 'warning' ? 'orange' : undefined}>{entry.severity}</Badge><span>{entry.message}</span><time>{new Date(entry.ts).toLocaleString()}</time></div>) : <div className="desktop-empty">No events recorded.</div>}</div></section></>; }

function NetworkPage({ settings, save }: Omit<PageProps, 'status'>) { const network = settings.network; const patch = (value: Partial<Settings['network']>) => void save({ ...settings, network: { ...network, ...value } }); return <SettingsCard title="Network" subtitle="Local Only is recommended and never exposes the Mac directly to the Internet."><div className="network-modes"><button type="button" aria-pressed={network.mode === 'local-only'} onClick={() => patch({ mode: 'local-only' })}><Icon name="wifi" /><b>Local Only</b><span>Direct iPad ↔ Mac connection on your private network.</span><Badge tone="green">Recommended</Badge></button><button type="button" aria-pressed={network.mode === 'remote-access'} onClick={() => patch({ mode: 'remote-access' })}><Icon name="globe" /><b>Remote Access</b><span>Architecture reserved for strongly authenticated relay access.</span><Badge>Not configured</Badge></button></div><Group title="Discovery"><Row title="Advertise this Mac nearby" accessory={<Switch label="Advertise this Mac nearby" checked={network.advertiseMdns} onChange={(advertiseMdns) => patch({ advertiseMdns })} />} /><Row title="Allow nearby pairing requests" subtitle="Approval on this Mac is always required" accessory={<Switch label="Allow nearby pairing" checked={network.allowNearbyPairing} onChange={(allowNearbyPairing) => patch({ allowNearbyPairing })} />} /><NumberSetting label="Secure local port" suffix="TCP" value={network.httpsPort} min={1024} max={65535} onChange={(httpsPort) => patch({ httpsPort })} /></Group></SettingsCard>; }

function AdvancedPage({ settings, save }: PageProps) { const advanced = settings.advanced; const patch = (value: Partial<Settings['advanced']>) => void save({ ...settings, advanced: { ...advanced, ...value } }); return <SettingsCard title="Advanced" subtitle="These controls can affect availability and troubleshooting. Secure defaults are recommended."><Group title="Startup"><Row title="Start service with Vision-Pro" accessory={<Switch label="Start service on launch" checked={advanced.startServiceOnLaunch} onChange={(startServiceOnLaunch) => patch({ startServiceOnLaunch })} />} /><Row title="Open at login" subtitle="Adds Vision-Pro to Login Items" accessory={<Switch label="Open at login" checked={advanced.launchAtLogin} onChange={(launchAtLogin) => patch({ launchAtLogin })} />} /></Group><Group title="Diagnostics"><Row title="Verbose local logging" accessory={<Switch label="Verbose local logging" checked={advanced.verboseLogging} onChange={(verboseLogging) => patch({ verboseLogging })} />} /><Row title="Developer loopback endpoint" subtitle="127.0.0.1 only; disabled in normal use" accessory={<Switch label="Developer loopback" checked={advanced.developerLoopback} onChange={(developerLoopback) => patch({ developerLoopback })} />} /></Group></SettingsCard>; }

function About({ status }: { status: AgentStatus }) { return <SettingsCard title="Vision-Pro" subtitle="Your computer. Everywhere."><div className="about-mark"><Icon name="devices" size={44} /><h2>Vision-Pro</h2><p>Development code name · Version {status.version}</p></div><Group title="This Mac"><Row title="Agent ID" value={<span className="mono">{status.agentId}</span>} /><Row title="Identity fingerprint" value={<span className="mono">{status.fingerprint}</span>} /><Row title="Local hostname" value={status.hostname} /></Group><p className="about-note">Vision-Pro uses a direct encrypted WebRTC path on the local network. Video does not transit through a cloud service in Local Only mode.</p></SettingsCard>; }

type PageProps = { status: AgentStatus; settings: Settings; save: (settings: Settings) => Promise<void> };
function SettingsCard({ title, subtitle, children }: { title: string; subtitle: string; children: React.ReactNode }) { return <section className="settings-layout"><div className="settings-intro"><h2>{title}</h2><p>{subtitle}</p></div><div className="settings-body">{children}</div></section>; }
function SettingSegment({ label, value, options, onChange }: { label: string; value: string; options: [string, string][]; onChange: (value: string) => void }) { return <div className="setting-segment"><span>{label}</span><Segmented value={value} options={options.map(([v, l]) => ({ value: v, label: l }))} onChange={onChange} ariaLabel={label} /></div>; }
function NumberSetting({ label, suffix, value, min, max, onChange }: { label: string; suffix: string; value: number; min: number; max: number; onChange: (value: number) => void }) { return <label className="number-setting"><span>{label}</span><span><input type="number" min={min} max={max} value={value} onChange={(event) => onChange(Number(event.currentTarget.value))} /><small>{suffix}</small></span></label>; }
function Summary({ icon, label, value, detail }: { icon: IconName; label: string; value: string; detail: string }) { return <article className="summary-card"><Icon name={icon} /><span>{label}</span><b>{value}</b><small>{detail}</small></article>; }
function Metric({ value, label }: { value: string; label: string }) { return <div className="live-metric"><b>{value}</b><span>{label}</span></div>; }
function CardTitle({ title, subtitle, action }: { title: string; subtitle: string; action?: React.ReactNode }) { return <header className="card-title"><div><h2>{title}</h2><p>{subtitle}</p></div>{action}</header>; }
function Empty({ icon, title, text }: { icon: IconName; title: string; text: string }) { return <div className="desktop-empty large"><Icon name={icon} size={38} /><h2>{title}</h2><p>{text}</p></div>; }

function PairingBanner({ request, onRespond }: { request: AgentStatus['pendingPairing'] & {}; onRespond: (approve: boolean) => Promise<void> }) { return <div className="pairing-banner"><div className="pairing-device"><span><Icon name="tablet" /></span><div><p>Connection request</p><h2>{request.deviceName}</h2><small>{request.deviceKind} · {request.ip} · {request.method}</small></div></div><div className="pairing-code"><small>VERIFY CODE</small><b>{request.sas}</b></div><div className="pairing-actions"><Button variant="danger" onClick={() => void onRespond(false)}>Decline</Button><Button variant="primary" onClick={() => void onRespond(true)}>Allow</Button></div></div>; }

function ConnectDialog({ info, onRefresh, onClose }: { info: ConnectInfo; onRefresh: () => Promise<void>; onClose: () => void }) {
  const src = `data:image/svg+xml,${encodeURIComponent(info.setupQrSvg ?? info.qrSvg)}`;
  return <div className="dialog-scrim"><section className="connect-dialog" role="dialog" aria-modal="true" aria-labelledby="connect-title"><div className="connect-head"><div><p className="eyebrow">Secure pairing</p><h2 id="connect-title">Connect your iPad</h2></div><IconButton icon="close" label="Close" onClick={onClose} /></div><div className="connect-grid"><div className="qr-wrap"><img src={src} alt="Vision-Pro secure pairing QR code" /></div><div className="connect-copy"><ol><li>Open Vision-Pro on your iPad.</li><li>Scan this QR code or enter the code below.</li><li>Confirm the matching security code on this Mac.</li></ol><div className="pair-code"><span>PAIRING CODE</span><b>{info.code}</b></div><p className="expires">Expires {formatRelative(info.expiresAt)}</p><Button icon="refresh" onClick={() => void onRefresh()}>New code</Button></div></div><div className="trust-note"><Icon name="shield" /><span><b>Discovery is not authorization.</b> This Mac grants no control until you explicitly approve the request.</span></div></section></div>;
}

const permissionRows: { kind: PermissionKind; key: keyof Permissions; icon: IconName; title: string; subtitle: string }[] = [
  { kind: 'screen-recording', key: 'screenRecording', icon: 'display', title: 'Screen Recording', subtitle: 'Required to stream this Mac’s display to the iPad' },
  { kind: 'accessibility', key: 'accessibility', icon: 'pointer', title: 'Accessibility', subtitle: 'Required for the iPad to control the pointer, keyboard and Apple Pencil' },
];

function PermissionsGroup({ permissions, onPermission }: { permissions: Permissions | null; onPermission: (kind: PermissionKind) => Promise<void> }) {
  return <Group title="Privacy & Security" footer="After enabling a permission in System Settings, quit and reopen Vision-Pro if macOS asks you to.">{permissionRows.map((row) => { const granted = permissions?.[row.key] ?? false; return <Row key={row.kind} icon={row.icon} title={row.title} subtitle={row.subtitle} accessory={granted ? <Badge tone="green" dot>Allowed</Badge> : <Button size="sm" variant="primary" onClick={() => void onPermission(row.kind)}>Allow…</Button>} />; })}<Row icon="network" title="Local Network" subtitle="macOS asks once when an iPad first connects" accessory={<Button size="sm" onClick={() => void api.openPermissionSettings('local-network')}>Open Settings</Button>} /></Group>;
}

function PermissionsBanner({ permissions, onOpen }: { permissions: Permissions; onOpen: () => void }) {
  const missing = [!permissions.screenRecording && 'Screen Recording', !permissions.accessibility && 'Accessibility'].filter((item): item is string => Boolean(item));
  return <div className="desktop-alert" data-tone="orange"><Icon name="shield" /><span><span>macOS permission needed:</span> {missing.map((item, i) => <span key={item}>{i > 0 && ' · '}<b>{item}</b></span>)}</span><Button size="sm" onClick={onOpen}>Review</Button></div>;
}
