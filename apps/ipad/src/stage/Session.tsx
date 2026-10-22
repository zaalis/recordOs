import {
  DOM_CODE_TO_KEY,
  InputEncoder,
  KEY_LABEL,
  VK,
  comboLabel,
  profileForExecutable,
  type AppProfile,
  type KeyControl,
  type KeyName,
  type ProfileControl,
} from '@vision-pro/protocol';
import { Badge, Button, Group, Icon, IconButton, Row, Slider, Switch, formatMs, useToast } from '@vision-pro/design';
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { connection, exportConnectionDiagnostics } from '../lib/connection.ts';
import { allProfiles, setScreen, updatePrefs, useApp } from '../lib/store.ts';
import type { ControlMode, Preferences } from '../lib/storage.ts';
import { isFullscreenActive, toggleFullscreen } from '../lib/platform.ts';
import { InputController, type InputSource, type ThreeDNavigation } from './input-controller.ts';

import { StyleSettings } from '../screens/StyleSettings.tsx';
import { PointerTrail } from './PointerTrail.tsx';

type Panel = 'displays' | 'control' | 'keyboard' | 'shortcuts' | 'settings' | null;
type Ripple = { id: number; kind: 'tap' | 'right' | 'drag'; x: number; y: number };

const encoder = new InputEncoder();
const keyboardKeys: KeyName[] = ['Ctrl', 'Alt', 'Shift', 'Win', 'Tab', 'Esc', 'Enter', 'Delete', 'Backspace', 'F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'F11', 'F12'];

/**
 * The part of the surface mapped to the PC display, with the display's proportions (like a
 * graphics tablet's "keep proportions" mode): a stroke on the iPad keeps its shape on the PC.
 */
function activeArea(surface: DOMRect, display: { width: number; height: number } | undefined): DOMRect {
  const margin = 18;
  const width = Math.max(1, surface.width - margin * 2);
  const height = Math.max(1, surface.height - margin * 2);
  const ratio = display && display.width > 0 && display.height > 0 ? display.width / display.height : width / height;
  const fitWidth = Math.min(width, height * ratio);
  const fitHeight = fitWidth / ratio;
  return new DOMRect(surface.left + (surface.width - fitWidth) / 2, surface.top + (surface.height - fitHeight) / 2, fitWidth, fitHeight);
}

export function Session() {
  const snapshot = useSyncExternalStore(connection.subscribe, connection.getSnapshot);
  const prefs = useApp((s) => s.prefs);
  const surfaceRef = useRef<HTMLDivElement>(null);
  const hiddenInputRef = useRef<HTMLInputElement>(null);
  const controllerRef = useRef<InputController | null>(null);
  const [panel, setPanel] = useState<Panel>(null);
  const [interfaceHidden, setInterfaceHidden] = useState(false);
  const [idle, setIdle] = useState(false);
  const [ripples, setRipples] = useState<Ripple[]>([]);
  const [latched, setLatched] = useState<Set<KeyName>>(new Set());
  const [toolbarDrag, setToolbarDrag] = useState<{ x: number; y: number } | null>(null);
  const [inputSource, setInputSource] = useState<InputSource>('fingers');
  const [fullscreen, setFullscreen] = useState(() => isFullscreenActive());
  const [area, setArea] = useState<DOMRect | null>(null);
  const activityTimer = useRef(0);
  const toast = useToast();
  const previousLink = useRef(snapshot.link);

  // Confirm the move when a plugged-in cable takes over the session (and when it is unplugged).
  useEffect(() => {
    const before = previousLink.current;
    previousLink.current = snapshot.link;
    if (!before || !snapshot.link || before === snapshot.link) return;
    if (snapshot.link === 'usb') toast('Commandes transmises par le câble USB', { icon: 'cable', tone: 'green' });
    else if (before === 'usb') toast('Câble débranché : les commandes repassent par le réseau', { icon: 'wifi' });
  }, [snapshot.link, toast]);

  const activeProfile = useMemo(() => {
    const profiles = allProfiles();
    if (prefs.autoProfiles && snapshot.activeApp?.exe) return profileForExecutable(snapshot.activeApp.exe, profiles) ?? profiles.find((p) => p.id === prefs.activeProfileId) ?? profiles[0]!;
    return profiles.find((p) => p.id === prefs.activeProfileId) ?? profiles[0]!;
  }, [prefs.activeProfileId, prefs.autoProfiles, snapshot.activeApp]);
  const threeDNavigation: ThreeDNavigation = activeProfile.id === 'blender' || activeProfile.id === 'maya' || activeProfile.id === 'zbrush' || activeProfile.id === 'houdini' ? activeProfile.id : 'none';
  const display = snapshot.displays.find((d) => d.id === snapshot.activeDisplayId) ?? snapshot.displays[0];

  const activity = useCallback(() => {
    // While controls are hidden, regular drawing gestures must stay focused
    // on the remote application. Only the dedicated reveal zone brings them
    // back, so the toolbar never obstructs an ongoing creation session.
    if (interfaceHidden) return;
    setIdle(false);
    window.clearTimeout(activityTimer.current);
    if (!panel) {
      activityTimer.current = window.setTimeout(() => {
        setIdle(true);
        setInterfaceHidden(true);
      }, 5000);
    }
  }, [interfaceHidden, panel]);

  const revealControls = useCallback(() => {
    setInterfaceHidden(false);
    setIdle(false);
    window.clearTimeout(activityTimer.current);
    if (!panel) {
      activityTimer.current = window.setTimeout(() => {
        setIdle(true);
        setInterfaceHidden(true);
      }, 5000);
    }
  }, [panel]);

  const contentRect = useCallback((): DOMRect | null => {
    const surface = surfaceRef.current?.getBoundingClientRect();
    return surface ? activeArea(surface, display) : null;
  }, [display]);

  const sendCombo = useCallback((keys: readonly KeyName[]) => {
    connection.sendInput(encoder.combo(keys.map((key) => VK[key])));
    activity();
  }, [activity]);

  // Draw the active area where input is mapped, and follow rotations and display changes.
  useEffect(() => {
    const surface = surfaceRef.current;
    if (!surface) return;
    const measure = () => setArea(contentRect());
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(surface);
    return () => observer.disconnect();
  }, [contentRect]);

  const inputContext = useRef({ contentRect, activity, sendCombo, prefs });
  inputContext.current = { contentRect, activity, sendCombo, prefs };
  useEffect(() => {
    const surface = surfaceRef.current;
    if (!surface) return;
    const controller = new InputController(
      surface,
      () => inputContext.current.contentRect(),
      { reliable: (buffer) => connection.sendInput(buffer), realtime: (buffer) => connection.sendRealtime(buffer) },
      prefs,
      {
        onToggleInterface: () => setInterfaceHidden((hidden) => !hidden),
        onFeedback: (kind, x, y) => {
          const id = Date.now() + Math.random();
          setRipples((items) => [...items.slice(-4), { id, kind, x, y }]);
          window.setTimeout(() => setRipples((items) => items.filter((item) => item.id !== id)), 500);
        },
        onUndo: () => inputContext.current.sendCombo(['Ctrl', 'Z']),
        onRedo: () => inputContext.current.sendCombo(['Ctrl', 'Shift', 'Z']),
        onActivity: () => inputContext.current.activity(),
        modifiersForClick: () => undefined,
        onInputSource: setInputSource,
      },
      threeDNavigation,
    );
    controller.mode = prefs.controlMode;
    controller.enabled = snapshot.controlAllowed;
    controllerRef.current = controller;
    const detach = controller.attach();
    return () => { detach(); controllerRef.current = null; };
  }, [threeDNavigation]);
  useEffect(() => {
    const controller = controllerRef.current;
    if (!controller) return;
    if (controller.mode !== prefs.controlMode || !snapshot.controlAllowed) controller.releaseAll();
    controller.mode = prefs.controlMode;
    controller.enabled = snapshot.controlAllowed;
    controller.setSettings(prefs);
  }, [prefs, snapshot.controlAllowed]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) return;
      const key = DOM_CODE_TO_KEY[event.code];
      if (!key || event.repeat) return;
      event.preventDefault();
      connection.sendInput(encoder.key(VK[key], event.type === 'keydown'));
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener('keyup', onKey);
    return () => { window.removeEventListener('keydown', onKey); window.removeEventListener('keyup', onKey); };
  }, []);

  useEffect(() => () => window.clearTimeout(activityTimer.current), []);

  // Start the five-second countdown as soon as the surface is ready.
  useEffect(() => {
    if (interfaceHidden || panel) {
      window.clearTimeout(activityTimer.current);
      return;
    }
    activity();
  }, [activity, interfaceHidden, panel]);

  useEffect(() => {
    const sync = () => setFullscreen(isFullscreenActive());
    document.addEventListener('fullscreenchange', sync);
    document.addEventListener('webkitfullscreenchange', sync);
    return () => {
      document.removeEventListener('fullscreenchange', sync);
      document.removeEventListener('webkitfullscreenchange', sync);
    };
  }, []);

  const choosePanel = (next: Exclude<Panel, null>) => {
    setInterfaceHidden(false);
    setPanel((current) => current === next ? null : next);
    activity();
  };

  const setMode = (mode: ControlMode) => {
    updatePrefs({ controlMode: mode });
    if (controllerRef.current) controllerRef.current.mode = mode;
  };

  const disconnect = () => {
    connection.disconnect();
    setScreen('home');
  };

  const toggleStageFullscreen = () => {
    void toggleFullscreen().then(setFullscreen);
  };

  const dragToolbar = (event: React.PointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    setToolbarDrag({ x: event.clientX, y: event.clientY });
  };

  const moveToolbar = (event: React.PointerEvent<HTMLButtonElement>) => {
    if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
    setToolbarDrag({ x: event.clientX, y: event.clientY });
  };

  const dropToolbar = (event: React.PointerEvent<HTMLButtonElement>) => {
    if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
    event.currentTarget.releasePointerCapture(event.pointerId);
    const distances = {
      top: event.clientY,
      right: innerWidth - event.clientX,
      bottom: innerHeight - event.clientY,
      left: event.clientX,
    };
    const edge = (Object.entries(distances).sort(([, a], [, b]) => a - b)[0]?.[0] ?? 'bottom') as Preferences['toolbar']['edge'];
    const offset = edge === 'top' || edge === 'bottom' ? event.clientX / innerWidth : event.clientY / innerHeight;
    updatePrefs({ toolbar: { ...prefs.toolbar, edge, offset: Math.max(0.12, Math.min(0.88, offset)) } });
    setToolbarDrag(null);
  };

  const nudgeToolbar = (event: React.KeyboardEvent<HTMLButtonElement>) => {
    const edgeForArrow: Partial<Record<string, Preferences['toolbar']['edge']>> = {
      ArrowUp: 'top',
      ArrowRight: 'right',
      ArrowDown: 'bottom',
      ArrowLeft: 'left',
    };
    const nextEdge = edgeForArrow[event.key];
    if (!nextEdge) return;
    event.preventDefault();
    const alongCurrentEdge =
      ((prefs.toolbar.edge === 'top' || prefs.toolbar.edge === 'bottom') && (event.key === 'ArrowLeft' || event.key === 'ArrowRight'))
      || ((prefs.toolbar.edge === 'left' || prefs.toolbar.edge === 'right') && (event.key === 'ArrowUp' || event.key === 'ArrowDown'));
    if (alongCurrentEdge) {
      const direction = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1 : -1;
      const step = event.shiftKey ? 0.1 : 0.04;
      updatePrefs({ toolbar: { ...prefs.toolbar, offset: Math.max(0.12, Math.min(0.88, prefs.toolbar.offset + direction * step)) } });
    } else {
      updatePrefs({ toolbar: { ...prefs.toolbar, edge: nextEdge } });
    }
  };

  const toolbarStyle = toolbarDrag
    ? { left: toolbarDrag.x, top: toolbarDrag.y, transform: 'translate(-50%, -50%)' }
    : toolbarPosition(prefs.toolbar.edge, prefs.toolbar.offset);
  const panelStyle = panelPosition(prefs.toolbar.edge);

  const surfaceStyle = prefs.surfaceStyle;
  const background = surfaceStyle.background === 'graphite' ? undefined : surfaceStyle.background === 'midnight' ? '#081323' : surfaceStyle.background === 'paper' ? '#ece9e2' : surfaceStyle.backgroundColor;
  const ink = surfaceStyle.patternColor + Math.round(surfaceStyle.patternOpacity * 255).toString(16).padStart(2, '0');
  const pattern = surfaceStyle.pattern === 'none' ? 'none' : surfaceStyle.pattern === 'grid' ? `linear-gradient(${ink} 1px, transparent 1px), linear-gradient(90deg, ${ink} 1px, transparent 1px)` : `radial-gradient(circle, ${ink} 0 0.65px, transparent 0.85px)`;
  return (
    <main className="stage" onPointerDown={activity}>
      <div className="tablet-surface" aria-hidden="true" style={background ? { background } : undefined} />
      {area && <div className="tablet-area" aria-hidden="true" style={{ left: area.left, top: area.top, width: area.width, height: area.height, backgroundImage: pattern, backgroundSize: `${surfaceStyle.patternSpacing}px ${surfaceStyle.patternSpacing}px` }} />}
      <div ref={surfaceRef} className="stage-surface" aria-label={`Surface de tablette graphique (${prefs.controlMode})`} />

      <PointerTrail target={surfaceRef} style={surfaceStyle} />
      {ripples.map((ripple) => <span key={ripple.id} className="touch-ripple" data-kind={ripple.kind} style={{ left: ripple.x, top: ripple.y }} />)}

      {!interfaceHidden && <div className="input-source-indicator vp-glass" aria-live="polite"><Icon name={inputSource === 'pencil' ? 'pencil' : inputSource === 'pointer' ? 'trackpad' : 'touch'} size={16} /><span>{threeDNavigation !== 'none' ? activeProfile.name : display?.name ?? 'Windows'}</span><span aria-hidden="true">·</span><span>{inputSource === 'pencil' ? 'Apple Pencil' : inputSource === 'pointer' ? 'Trackpad ou souris' : 'Doigts'}</span></div>}
      {!snapshot.controlAllowed && <div className="banner vp-glass"><Icon name="eye" size={18} /> Commandes désactivées sur le PC</div>}
      {snapshot.phase === 'reconnecting' && !snapshot.switching && <div className="banner vp-glass"><span className="vp-spinner" /> Reconnexion sécurisée…</div>}

      {prefs.showStats && <Stats state={snapshot} />}
      {panel && !interfaceHidden && <PanelView panel={panel} profile={activeProfile} prefs={prefs} state={snapshot} onClose={() => setPanel(null)} onMode={setMode} onCombo={sendCombo} onTextFocus={() => hiddenInputRef.current?.focus()} style={panelStyle} latched={latched} setLatched={setLatched} />}

      {!interfaceHidden && (
        <nav className="toolbar" aria-label="Session controls" data-dragging={toolbarDrag ? 'true' : undefined} data-idle={idle ? 'true' : undefined} data-vertical={prefs.toolbar.edge === 'left' || prefs.toolbar.edge === 'right' ? 'true' : undefined} style={toolbarStyle}>
          <button type="button" className="toolbar-grip" aria-label="Move toolbar. Drag, or use arrow keys." onKeyDown={nudgeToolbar} onPointerDown={dragToolbar} onPointerMove={moveToolbar} onPointerUp={dropToolbar} onPointerCancel={() => setToolbarDrag(null)}><Icon name="grip" size={16} /></button>
          <ToolbarButton icon="home" label="Home" onClick={disconnect} />
          <span className="toolbar-divider" />
          <ToolbarButton icon="displays" label="Displays" active={panel === 'displays'} onClick={() => choosePanel('displays')} />
          <ToolbarButton icon={prefs.controlMode === 'pencil' ? 'pencil' : prefs.controlMode === 'trackpad' ? 'trackpad' : 'touch'} label="Control" active={panel === 'control'} onClick={() => choosePanel('control')} />
          <ToolbarButton icon="keyboard" label="Keyboard" active={panel === 'keyboard'} onClick={() => choosePanel('keyboard')} />
          <ToolbarButton icon="command" label="Shortcuts" active={panel === 'shortcuts'} onClick={() => choosePanel('shortcuts')} />
          <ToolbarButton icon="gear" label="Settings" active={panel === 'settings'} onClick={() => choosePanel('settings')} />
          <ToolbarButton icon={fullscreen ? 'collapse' : 'fullscreen'} label={fullscreen ? 'Exit fullscreen' : 'Enter fullscreen'} active={fullscreen} onClick={toggleStageFullscreen} />
          <ToolbarButton icon="collapse" label="Hide controls" onClick={() => { window.clearTimeout(activityTimer.current); setPanel(null); setIdle(true); setInterfaceHidden(true); }} />
        </nav>
      )}
      {interfaceHidden && <button className="toolbar-reveal-zone" aria-label="Afficher les commandes de session" style={toolbarStyle} onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); }} onClick={(event) => { event.stopPropagation(); revealControls(); }} />}

      <input
        ref={hiddenInputRef}
        className="hidden-input"
        aria-label="Type on Windows"
        autoCapitalize="none"
        autoCorrect="off"
        onInput={(event) => {
          const value = event.currentTarget.value;
          if (value) encoder.text(value).forEach((buffer) => connection.sendInput(buffer));
          event.currentTarget.value = '';
        }}
      />
    </main>
  );
}

function ToolbarButton({ icon, label, active, onClick }: { icon: Parameters<typeof Icon>[0]['name']; label: string; active?: boolean; onClick: () => void }) {
  return <button type="button" className="toolbar-button" aria-label={label} aria-pressed={active} onClick={onClick}><Icon name={icon} /><span className="label">{label}</span></button>;
}

function PanelView({ panel, profile, prefs, state, onClose, onMode, onCombo, onTextFocus, style, latched, setLatched }: {
  panel: Exclude<Panel, null>;
  profile: AppProfile;
  prefs: Preferences;
  state: ReturnType<typeof connection.getSnapshot>;
  onClose: () => void;
  onMode: (mode: ControlMode) => void;
  onCombo: (keys: readonly KeyName[]) => void;
  onTextFocus: () => void;
  style: React.CSSProperties;
  latched: Set<KeyName>;
  setLatched: React.Dispatch<React.SetStateAction<Set<KeyName>>>;
}) {
  const titles = { displays: 'Écrans', control: 'Control', keyboard: 'Keyboard', shortcuts: profile.name, settings: 'Session' };
  return (
    <aside className="panel" data-wide={panel === 'shortcuts' ? 'true' : undefined} style={style} aria-label={titles[panel]}>
      <header className="panel-header"><h3>{titles[panel]}</h3><IconButton icon="close" label="Close" size="sm" onClick={onClose} /></header>
      <div className="panel-body">
        {panel === 'displays' && <DisplaysPanel state={state} />}
        {panel === 'control' && <ControlPanel prefs={prefs} onMode={onMode} />}
        {panel === 'keyboard' && <KeyboardPanel onCombo={onCombo} onTextFocus={onTextFocus} latched={latched} setLatched={setLatched} />}
        {panel === 'shortcuts' && <ShortcutsPanel profile={profile} onCombo={onCombo} latched={latched} setLatched={setLatched} />}
        {panel === 'settings' && <SessionSettings prefs={prefs} state={state} />}
      </div>
    </aside>
  );
}

function DisplaysPanel({ state }: { state: ReturnType<typeof connection.getSnapshot> }) {
  return <Group footer="La surface de l’iPad correspond à l’écran choisi, avec ses proportions.">{state.displays.map((display) => <Row key={display.id} icon="display" title={display.name} subtitle={`${display.width} × ${display.height}`} accessory={display.id === state.activeDisplayId ? <Badge tone="green" dot>Actif</Badge> : <Button size="sm" onClick={() => connection.selectDisplay(display.id)}>Utiliser</Button>} />)}</Group>;
}

function ControlPanel({ prefs, onMode }: { prefs: Preferences; onMode: (mode: ControlMode) => void }) {
  return <>
    <div className="mode-cards">{([['direct', 'touch', 'Direct Touch'], ['trackpad', 'trackpad', 'Trackpad'], ['pencil', 'pencil', 'Pencil']] as const).map(([mode, icon, label]) => <button key={mode} type="button" className="mode-card" aria-pressed={prefs.controlMode === mode} onClick={() => onMode(mode)}><Icon name={icon} />{label}</button>)}</div>
    <Group title="Pointer">
      <div className="field-stack"><span>Tracking speed</span><Slider label="Tracking speed" min={0.4} max={2.4} step={0.1} value={prefs.trackingSpeed} onChange={(trackingSpeed) => updatePrefs({ trackingSpeed })} /></div>
      <div className="field-stack"><span>Scroll speed</span><Slider label="Scroll speed" min={0.4} max={2.4} step={0.1} value={prefs.scrollSpeed} onChange={(scrollSpeed) => updatePrefs({ scrollSpeed })} /></div>
      <Row title="Natural scrolling" accessory={<Switch label="Natural scrolling" checked={prefs.naturalScrolling} onChange={(naturalScrolling) => updatePrefs({ naturalScrolling })} />} />
      <Row title="Tap to click" accessory={<Switch label="Tap to click" checked={prefs.tapToClick} onChange={(tapToClick) => updatePrefs({ tapToClick })} />} />
    </Group>
    {prefs.controlMode === 'pencil' && <Group title="Apple Pencil"><Row title="Windows Ink" accessory={<Switch label="Windows Ink" checked={prefs.pencilWindowsInk} onChange={(pencilWindowsInk) => updatePrefs({ pencilWindowsInk })} />} /><Row title="Pencil hover" accessory={<Switch label="Pencil hover" checked={prefs.pencilHover} onChange={(pencilHover) => updatePrefs({ pencilHover })} />} /></Group>}
  </>;
}

function KeyboardPanel({ onCombo, onTextFocus, latched, setLatched }: { onCombo: (keys: readonly KeyName[]) => void; onTextFocus: () => void; latched: Set<KeyName>; setLatched: React.Dispatch<React.SetStateAction<Set<KeyName>>> }) {
  const press = (key: KeyName) => {
    if (['Ctrl', 'Alt', 'Shift', 'Win'].includes(key)) {
      const next = new Set(latched);
      if (next.has(key)) { connection.sendInput(encoder.key(VK[key], false)); next.delete(key); } else { connection.sendInput(encoder.key(VK[key], true)); next.add(key); }
      setLatched(next);
    } else onCombo([...latched, key]);
  };
  return <><Button icon="keyboard" variant="primary" block onClick={onTextFocus}>Open iPad keyboard</Button><div className="key-grid">{keyboardKeys.map((key) => <button key={key} type="button" className="key" data-latched={latched.has(key) ? 'true' : undefined} onClick={() => press(key)}>{KEY_LABEL[key] ?? key}</button>)}</div></>;
}

function ShortcutsPanel({ profile, onCombo, latched, setLatched }: { profile: AppProfile; onCombo: (keys: readonly KeyName[]) => void; latched: Set<KeyName>; setLatched: React.Dispatch<React.SetStateAction<Set<KeyName>>> }) {
  const trigger = (control: ProfileControl, direction: 'decrease' | 'increase' = 'increase') => {
    const keys = control.type === 'stepper' ? control[direction] : control.keys;
    if (control.type === 'key' && control.mode === 'latch' && keys.length === 1) {
      const key = keys[0]!; const next = new Set(latched); const down = !next.has(key);
      connection.sendInput(encoder.key(VK[key], down)); down ? next.add(key) : next.delete(key); setLatched(next); return;
    }
    onCombo([...latched, ...keys]);
  };
  return <>{profile.sections.map((section) => <section key={section.id}><div className="vp-overline" style={{ marginBottom: 8 }}>{section.title}</div><div className="key-grid">{section.controls.map((control) => control.type === 'stepper' ? <div className="stepper" key={control.id}><button type="button" aria-label={`Decrease ${control.label}`} onClick={() => trigger(control, 'decrease')}><Icon name="minus" /></button><span>{control.label}<small>{control.hint}</small></span><button type="button" aria-label={`Increase ${control.label}`} onClick={() => trigger(control, 'increase')}><Icon name="plus" /></button></div> : <ShortcutKey key={control.id} control={control} active={control.keys.some((key) => latched.has(key))} onTrigger={() => trigger(control)} />)}</div></section>)}</>;
}

function ShortcutKey({ control, active, onTrigger }: { control: KeyControl; active: boolean; onTrigger: () => void }) {
  return <button type="button" className="key" data-wide={control.wide ? 'true' : undefined} data-latched={active && control.mode === 'latch' ? 'true' : undefined} onClick={onTrigger}>{control.label}<small>{control.hint ?? comboLabel(control.keys)}</small></button>;
}

function linkLabel(state: ReturnType<typeof connection.getSnapshot>): string {
  if (state.link === 'usb') return 'Câble USB';
  if (state.link === 'relay') return 'Relais Internet';
  return /^100\.(6[4-9]|[7-9]\d|1[01]\d|12[0-7])\./.test(location.hostname) ? 'Tailscale' : 'Réseau local';
}

function SessionSettings({ prefs, state }: { prefs: Preferences; state: ReturnType<typeof connection.getSnapshot> }) {
  return <>
    <Group title="Interface"><Row icon="stats" title="Latence affichée" accessory={<Switch label="Latence affichée" checked={prefs.showStats} onChange={(showStats) => updatePrefs({ showStats })} />} /><Row icon="collapse" title="Commandes masquées après 5 secondes" value="Activé" /><Row icon="fullscreen" title="Plein écran" onClick={() => void toggleFullscreen()} chevron /></Group>
    <StyleSettings />
    <Group title="Connexion"><Row icon="shield" title="Chiffrement" value="TLS + bout en bout" /><Row icon={state.link === 'usb' ? 'cable' : 'network'} title="Chemin" value={linkLabel(state)} /><Row icon="gauge" title="Aller estimé (RTT ÷ 2)" value={formatMs(state.stats.inputLatencyMs)} /><Row icon="clock" title="Session" value={state.session?.expiresAt ? `jusqu’à ${new Date(state.session.expiresAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}` : 'Active'} /></Group>
    <Button variant="ghost" block onClick={exportConnectionDiagnostics}>Exporter le diagnostic de connexion</Button>
    <Button variant="danger" icon="power" block onClick={() => { connection.disconnect(); setScreen('home'); }}>Déconnecter</Button>
  </>;
}

function Stats({ state }: { state: ReturnType<typeof connection.getSnapshot> }) {
  const stats = state.stats;
  return <aside className="stats-overlay" aria-label="Connection statistics"><div className="stats-title"><span className="vp-dot" data-tone="green" /> Live</div><dl><dt>Ping</dt><dd>{formatMs(stats.pingMs)}</dd><dt>Aller estimé</dt><dd>{formatMs(stats.inputLatencyMs)}</dd><dt>File réseau</dt><dd>{connection.diagnostics().bufferedBytes} octets</dd><dt>Reconnexions</dt><dd>{connection.counters.reconnects}</dd><dt>Chemin</dt><dd>{linkLabel(state)}</dd></dl></aside>;
}

function toolbarPosition(edge: Preferences['toolbar']['edge'], offset: number): React.CSSProperties {
  const pct = `${Math.round(Math.max(0.12, Math.min(0.88, offset)) * 100)}%`;
  if (edge === 'top') return { top: 'calc(var(--safe-top) + 14px)', left: pct, transform: 'translateX(-50%)' };
  if (edge === 'left') return { left: 'calc(var(--safe-left) + 14px)', top: pct, transform: 'translateY(-50%)' };
  if (edge === 'right') return { right: 'calc(var(--safe-right) + 14px)', top: pct, transform: 'translateY(-50%)' };
  return { bottom: 'calc(var(--safe-bottom) + 14px)', left: pct, transform: 'translateX(-50%)' };
}

function panelPosition(edge: Preferences['toolbar']['edge']): React.CSSProperties {
  if (edge === 'top') return { top: 'calc(var(--safe-top) + 88px)', left: '50%', ['--vp-panel-base' as string]: 'translateX(-50%)' };
  if (edge === 'left') return { left: 'calc(var(--safe-left) + 88px)', top: '50%', ['--vp-panel-base' as string]: 'translateY(-50%)' };
  if (edge === 'right') return { right: 'calc(var(--safe-right) + 88px)', top: '50%', ['--vp-panel-base' as string]: 'translateY(-50%)' };
  return { bottom: 'calc(var(--safe-bottom) + 88px)', left: '50%', ['--vp-panel-base' as string]: 'translateX(-50%)' };
}
