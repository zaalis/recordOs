import {
  BITRATE_PRESETS_KBPS,
  DOM_CODE_TO_KEY,
  InputEncoder,
  KEY_LABEL,
  LATENCY_PROFILES,
  MODIFIER_KEYS,
  VK,
  comboLabel,
  profileForApp,
  type AppProfile,
  type KeyControl,
  type KeyName,
  type LatencyProfile,
  type ProfileControl,
  type VideoSettings,
} from '@vision-pro/protocol';
import { Badge, Button, Group, Icon, IconButton, Row, Segmented, Slider, Switch, formatBitrate, formatMs } from '@vision-pro/design';
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { connection } from '../lib/connection.ts';
import { allProfiles, setScreen, store, updatePrefs, useApp } from '../lib/store.ts';
import type { ControlMode, Preferences, StageMode } from '../lib/storage.ts';
import { isFullscreenActive, toggleFullscreen } from '../lib/platform.ts';
import { InputController, type InputSource, type ThreeDNavigation } from './input-controller.ts';
import { ViewTransform, type ViewState } from './view.ts';

type Panel = 'displays' | 'control' | 'keyboard' | 'shortcuts' | 'quality' | 'settings' | null;
type Ripple = { id: number; kind: 'tap' | 'right' | 'drag'; x: number; y: number };

const encoder = new InputEncoder();
const keyboardKeys: KeyName[] = ['Cmd', 'Option', 'Control', 'Shift', 'Tab', 'Esc', 'Enter', 'Backspace', 'Delete', 'Up', 'Down', 'Left', 'Right', 'F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'F11', 'F12'];

export function Session() {
  const snapshot = useSyncExternalStore(connection.subscribe, connection.getSnapshot);
  const prefs = useApp((s) => s.prefs);
  const surfaceRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLDivElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const hiddenInputRef = useRef<HTMLInputElement>(null);
  const viewRef = useRef(new ViewTransform());
  const controllerRef = useRef<InputController | null>(null);
  const [view, setView] = useState<ViewState>(viewRef.current.state);
  const [panel, setPanel] = useState<Panel>(null);
  const [interfaceHidden, setInterfaceHidden] = useState(false);
  const [idle, setIdle] = useState(false);
  const [ripples, setRipples] = useState<Ripple[]>([]);
  const [cursor, setCursor] = useState({ x: 0.5, y: 0.5, visible: false, shapeId: 0 });
  const [latched, setLatched] = useState<Set<KeyName>>(new Set());
  const [toolbarDrag, setToolbarDrag] = useState<{ x: number; y: number } | null>(null);
  const [inputSource, setInputSource] = useState<InputSource>('fingers');
  const [fullscreen, setFullscreen] = useState(() => isFullscreenActive());
  const activityTimer = useRef(0);

  const activeProfile = useMemo(() => {
    const profiles = allProfiles();
    if (prefs.autoProfiles && snapshot.activeApp?.exe) return profileForApp(snapshot.activeApp.exe, profiles) ?? profiles.find((p) => p.id === prefs.activeProfileId) ?? profiles[0]!;
    return profiles.find((p) => p.id === prefs.activeProfileId) ?? profiles[0]!;
  }, [prefs.activeProfileId, prefs.autoProfiles, snapshot.activeApp]);
  const threeDNavigation: ThreeDNavigation = activeProfile.id === 'blender' || activeProfile.id === 'maya' || activeProfile.id === 'zbrush' || activeProfile.id === 'houdini' ? activeProfile.id : 'none';

  const activity = useCallback(() => {
    // While controls are hidden, regular modelling gestures must stay focused
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
    if (prefs.stageMode === 'tablet') return surfaceRef.current?.getBoundingClientRect() ?? null;
    const video = videoRef.current;
    if (!video) return null;
    const box = video.getBoundingClientRect();
    const sourceRatio = (video.videoWidth || snapshot.video?.width || 16) / (video.videoHeight || snapshot.video?.height || 9);
    const boxRatio = box.width / Math.max(1, box.height);
    if (boxRatio > sourceRatio) {
      const width = box.height * sourceRatio;
      return new DOMRect(box.left + (box.width - width) / 2, box.top, width, box.height);
    }
    const height = box.width / sourceRatio;
    return new DOMRect(box.left, box.top + (box.height - height) / 2, box.width, height);
  }, [prefs.stageMode, snapshot.video?.height, snapshot.video?.width]);

  const sendCombo = useCallback((keys: readonly KeyName[]) => {
    connection.sendInput(encoder.combo(keys.map((key) => VK[key])));
    activity();
  }, [activity]);

  useEffect(() => viewRef.current.subscribe(setView), []);
  useEffect(() => {
    const resize = () => {
      const box = surfaceRef.current?.getBoundingClientRect();
      if (box) viewRef.current.setBounds(box.width, box.height);
    };
    resize();
    const observer = new ResizeObserver(resize);
    if (surfaceRef.current) observer.observe(surfaceRef.current);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const video = videoRef.current;
    if (!video || !snapshot.stream) return;
    video.srcObject = snapshot.stream;
    const resume = () => {
      if (!document.hidden && video.srcObject === snapshot.stream && video.paused) {
        void video.play().catch(() => undefined);
      }
    };
    resume();
    video.addEventListener('loadedmetadata', resume);
    document.addEventListener('visibilitychange', resume);
    return () => {
      video.removeEventListener('loadedmetadata', resume);
      document.removeEventListener('visibilitychange', resume);
      if (video.srcObject === snapshot.stream) video.srcObject = null;
    };
  }, [snapshot.stream]);

  useEffect(() => {
    connection.onCursor = (next) => setCursor(next);
    connection.onCursorShape = () => setCursor((current) => ({ ...current }));
    return () => { connection.onCursor = undefined; connection.onCursorShape = undefined; };
  }, []);

  useEffect(() => {
    const surface = surfaceRef.current;
    if (!surface) return;
    const controller = new InputController(
      surface,
      contentRect,
      { reliable: (buffer) => connection.sendInput(buffer), realtime: (buffer) => connection.sendRealtime(buffer) },
      viewRef.current,
      prefs,
      {
        onToggleInterface: () => setInterfaceHidden((hidden) => !hidden),
        onFeedback: (kind, x, y) => {
          const id = Date.now() + Math.random();
          setRipples((items) => [...items.slice(-4), { id, kind, x, y }]);
          window.setTimeout(() => setRipples((items) => items.filter((item) => item.id !== id)), 500);
        },
        onLocalCursorMove: (dx, dy) => setCursor((current) => ({ ...current, x: Math.max(0, Math.min(1, current.x + dx)), y: Math.max(0, Math.min(1, current.y + dy)), visible: true })),
        onUndo: () => sendCombo(['Cmd', 'Z']),
        onRedo: () => sendCombo(['Cmd', 'Shift', 'Z']),
        onActivity: activity,
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
  }, [activity, contentRect, prefs, sendCombo, snapshot.controlAllowed, threeDNavigation]);

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

  // Start the five-second countdown as soon as the remote stage is ready.
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

  const setStageMode = (stageMode: StageMode) => {
    updatePrefs({ stageMode });
    // A screenless tablet has a one-to-one active area. Resetting the view
    // prevents a prior pinch/pan from shifting the input coordinate space.
    if (stageMode === 'tablet') viewRef.current.reset();
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

  const cursorShape = connection.getCursorShape(cursor.shapeId);
  const toolbarStyle = toolbarDrag
    ? { left: toolbarDrag.x, top: toolbarDrag.y, transform: 'translate(-50%, -50%)' }
    : toolbarPosition(prefs.toolbar.edge, prefs.toolbar.offset);
  const panelStyle = panelPosition(prefs.toolbar.edge);

  return (
    <main className="stage" onPointerDown={activity}>
      {prefs.stageMode === 'stream' && <>
        <div ref={canvasRef} className="stage-canvas" style={{ transform: `translate3d(${view.x}px, ${view.y}px, 0) scale(${view.scale})` }}>
          <video ref={videoRef} className="stage-video" playsInline muted />
        </div>
        {!snapshot.stream && <div className="stage-waiting"><Icon name="display" size={34} /><span>Waiting for the first secure video frame…</span></div>}
      </>}
      {prefs.stageMode === 'tablet' && <div className="tablet-surface" aria-hidden="true" />}
      <div ref={surfaceRef} className="stage-surface" aria-label={prefs.stageMode === 'tablet' ? 'Surface de tablette graphique' : `${prefs.controlMode} remote control surface`} />

      {prefs.stageMode === 'stream' && prefs.showLocalCursor && cursor.visible && (
        cursorShape
          ? <img className="remote-cursor" src={cursorShape.url} alt="" style={{ width: cursorShape.width, height: cursorShape.height, transform: `translate(${cursor.x * innerWidth - cursorShape.hotX}px, ${cursor.y * innerHeight - cursorShape.hotY}px)` }} />
          : <div className="remote-cursor" style={{ transform: `translate(${cursor.x * innerWidth}px, ${cursor.y * innerHeight}px)` }}><Icon name="pointer" size={20} /></div>
      )}
      {ripples.map((ripple) => <span key={ripple.id} className="touch-ripple" data-kind={ripple.kind} style={{ left: ripple.x, top: ripple.y }} />)}

      {prefs.stageMode === 'stream' && view.scale > 1.01 && <div className="zoom-indicator vp-glass">{Math.round(view.scale * 100)}% <IconButton icon="close" label="Reset zoom" size="sm" onClick={() => viewRef.current.reset()} /></div>}
      {prefs.stageMode === 'stream' && <div className="input-source-indicator vp-glass" aria-live="polite"><Icon name={inputSource === 'pencil' ? 'pencil' : inputSource === 'pointer' ? 'trackpad' : 'touch'} size={16} /><span>{threeDNavigation !== 'none' ? activeProfile.name : 'macOS'}</span><span aria-hidden="true">·</span><span>{inputSource === 'pencil' ? 'Apple Pencil' : inputSource === 'pointer' ? 'Trackpad or mouse' : 'Fingers'}</span></div>}
      {!snapshot.controlAllowed && <div className="banner vp-glass"><Icon name="eye" size={18} /> View only — input is disabled on the Mac</div>}
      {snapshot.phase === 'reconnecting' && <div className="banner vp-glass"><span className="vp-spinner" /> Reconnecting securely…</div>}

      {prefs.showStats && <Stats state={snapshot} />}
      {panel && !interfaceHidden && <PanelView panel={panel} profile={activeProfile} prefs={prefs} state={snapshot} onClose={() => setPanel(null)} onMode={setMode} onStageMode={setStageMode} onCombo={sendCombo} onTextFocus={() => hiddenInputRef.current?.focus()} style={panelStyle} latched={latched} setLatched={setLatched} />}

      {!interfaceHidden && (
        <nav className="toolbar" aria-label="Session controls" data-dragging={toolbarDrag ? 'true' : undefined} data-idle={idle ? 'true' : undefined} data-vertical={prefs.toolbar.edge === 'left' || prefs.toolbar.edge === 'right' ? 'true' : undefined} style={toolbarStyle}>
          <button type="button" className="toolbar-grip" aria-label="Move toolbar. Drag, or use arrow keys." onKeyDown={nudgeToolbar} onPointerDown={dragToolbar} onPointerMove={moveToolbar} onPointerUp={dropToolbar} onPointerCancel={() => setToolbarDrag(null)}><Icon name="grip" size={16} /></button>
          <ToolbarButton icon="home" label="Home" onClick={disconnect} />
          <span className="toolbar-divider" />
          <ToolbarButton icon="displays" label="Displays" active={panel === 'displays'} onClick={() => choosePanel('displays')} />
          <ToolbarButton icon={prefs.controlMode === 'pencil' ? 'pencil' : prefs.controlMode === 'trackpad' ? 'trackpad' : 'touch'} label="Control" active={panel === 'control'} onClick={() => choosePanel('control')} />
          <ToolbarButton icon="keyboard" label="Keyboard" active={panel === 'keyboard'} onClick={() => choosePanel('keyboard')} />
          <ToolbarButton icon="command" label="Shortcuts" active={panel === 'shortcuts'} onClick={() => choosePanel('shortcuts')} />
          <ToolbarButton icon="gauge" label="Quality" active={panel === 'quality'} onClick={() => choosePanel('quality')} />
          <ToolbarButton icon="gear" label="Settings" active={panel === 'settings'} onClick={() => choosePanel('settings')} />
          <ToolbarButton icon={fullscreen ? 'collapse' : 'fullscreen'} label={fullscreen ? 'Exit fullscreen' : 'Enter fullscreen'} active={fullscreen} onClick={toggleStageFullscreen} />
          <ToolbarButton icon="collapse" label="Hide controls" onClick={() => { window.clearTimeout(activityTimer.current); setPanel(null); setIdle(true); setInterfaceHidden(true); }} />
        </nav>
      )}
      {interfaceHidden && <button className="toolbar-reveal-zone" aria-label="Afficher les commandes de session" style={toolbarStyle} onPointerDown={(event) => { event.stopPropagation(); revealControls(); }} />}

      <input
        ref={hiddenInputRef}
        className="hidden-input"
        aria-label="Type on the Mac"
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

function PanelView({ panel, profile, prefs, state, onClose, onMode, onStageMode, onCombo, onTextFocus, style, latched, setLatched }: {
  panel: Exclude<Panel, null>;
  profile: AppProfile;
  prefs: Preferences;
  state: ReturnType<typeof connection.getSnapshot>;
  onClose: () => void;
  onMode: (mode: ControlMode) => void;
  onStageMode: (mode: StageMode) => void;
  onCombo: (keys: readonly KeyName[]) => void;
  onTextFocus: () => void;
  style: React.CSSProperties;
  latched: Set<KeyName>;
  setLatched: React.Dispatch<React.SetStateAction<Set<KeyName>>>;
}) {
  const titles = { displays: 'Displays', control: 'Control', keyboard: 'Keyboard', shortcuts: profile.name, quality: 'Streaming', settings: 'Session' };
  return (
    <aside className="panel" data-wide={panel === 'shortcuts' || panel === 'quality' ? 'true' : undefined} style={style} aria-label={titles[panel]}>
      <header className="panel-header"><h3>{titles[panel]}</h3><IconButton icon="close" label="Close" size="sm" onClick={onClose} /></header>
      <div className="panel-body">
        {panel === 'displays' && <DisplaysPanel state={state} />}
        {panel === 'control' && <ControlPanel prefs={prefs} onMode={onMode} />}
        {panel === 'keyboard' && <KeyboardPanel onCombo={onCombo} onTextFocus={onTextFocus} latched={latched} setLatched={setLatched} />}
        {panel === 'shortcuts' && <ShortcutsPanel profile={profile} onCombo={onCombo} latched={latched} setLatched={setLatched} />}
        {panel === 'quality' && <QualityPanel prefs={prefs} state={state} />}
      {panel === 'settings' && <SessionSettings prefs={prefs} state={state} onStageMode={onStageMode} />}
      </div>
    </aside>
  );
}

function DisplaysPanel({ state }: { state: ReturnType<typeof connection.getSnapshot> }) {
  return <Group footer="Switching displays requests a fresh keyframe; input remains mapped to the selected display.">{state.displays.map((display) => <Row key={display.id} icon="display" title={display.name} subtitle={`${display.width} × ${display.height} · ${display.refreshHz} Hz`} accessory={display.id === state.activeDisplayId ? <Badge tone="green" dot>Active</Badge> : <Button size="sm" onClick={() => connection.selectDisplay(display.id)}>Use</Button>} />)}</Group>;
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
    {prefs.controlMode === 'pencil' && <Group title="Apple Pencil"><Row title="Pressure & tilt" subtitle="macOS tablet pen" accessory={<Switch label="Pressure & tilt" checked={prefs.pencilTablet} onChange={(pencilTablet) => updatePrefs({ pencilTablet })} />} /><Row title="Pencil hover" accessory={<Switch label="Pencil hover" checked={prefs.pencilHover} onChange={(pencilHover) => updatePrefs({ pencilHover })} />} /></Group>}
  </>;
}

function KeyboardPanel({ onCombo, onTextFocus, latched, setLatched }: { onCombo: (keys: readonly KeyName[]) => void; onTextFocus: () => void; latched: Set<KeyName>; setLatched: React.Dispatch<React.SetStateAction<Set<KeyName>>> }) {
  const press = (key: KeyName) => {
    if (MODIFIER_KEYS.includes(key)) {
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

function QualityPanel({ prefs, state }: { prefs: Preferences; state: ReturnType<typeof connection.getSnapshot> }) {
  const updateVideo = (patch: Partial<VideoSettings>) => { const video = { ...prefs.video, ...patch }; updatePrefs({ video }); connection.updateVideo(video); };
  return <>
    <div className="latency-cards">{Object.values(LATENCY_PROFILES).map((profile) => <button key={profile.id} type="button" className="latency-card" aria-pressed={prefs.video.profile === profile.id} onClick={() => updateVideo({ profile: profile.id })}><b>{profile.name}</b><small>{profile.summary}</small></button>)}</div>
    <Group title="Video">
      <div className="field-stack"><span>Resolution</span><Segmented block ariaLabel="Resolution" value={prefs.video.resolution} options={['auto', '720p', '1080p', '1440p', '2160p', '4320p', 'native'].map((value) => ({ value, label: value === 'auto' ? 'Auto' : value === '1440p' ? '2K' : value === '2160p' ? '4K' : value === '4320p' ? '8K' : value === 'native' ? 'Native' : value }))} onChange={(resolution) => updateVideo({ resolution: resolution as VideoSettings['resolution'] })} /></div>
      <div className="field-stack"><span>Frame rate</span><Segmented block ariaLabel="Frame rate" value={String(prefs.video.fps)} options={['auto', '30', '60', '90', '120'].map((value) => ({ value, label: value === 'auto' ? 'Auto' : value }))} onChange={(fps) => updateVideo({ fps: fps === 'auto' ? 'auto' : Number(fps) as 30 | 60 | 90 | 120 })} /></div>
      <div className="field-stack"><span>Codec</span><Segmented block ariaLabel="Codec" value={prefs.video.codec} options={['auto', 'h264', 'hevc', 'av1'].map((value) => ({ value, label: value === 'auto' ? 'Auto' : value.toUpperCase() }))} onChange={(codec) => updateVideo({ codec: codec as VideoSettings['codec'] })} /></div>
    </Group>
    {state.video && <div className="metric-grid"><Metric label="Resolution" value={`${state.video.width} × ${state.video.height}`} /><Metric label="Frame rate" value={`${state.video.fps} FPS`} /><Metric label="Bitrate" value={formatBitrate(state.video.targetKbps)} /><Metric label="Codec" value={state.video.codec.toUpperCase()} /><Metric label="Encoder" value={state.video.hardware ? 'Hardware' : 'Software'} /><Metric label="Network" value={formatBitrate(state.stats.networkKbps ?? undefined)} /></div>}
  </>;
}

function SessionSettings({ prefs, state, onStageMode }: { prefs: Preferences; state: ReturnType<typeof connection.getSnapshot>; onStageMode: (mode: StageMode) => void }) {
  return <>
    <Group title="Affichage"><div className="field-stack"><span>Surface de travail</span><Segmented block ariaLabel="Surface de travail" value={prefs.stageMode} options={[{ value: 'stream', label: 'Écran du Mac' }, { value: 'tablet', label: 'Tablette graphique' }]} onChange={(mode) => onStageMode(mode as StageMode)} /></div><p className="vp-footnote">Tablette graphique masque l’image du Mac et conserve les gestes de contrôle sur une surface noire à points fins.</p></Group>
    <Group title="Interface"><Row icon="stats" title="Statistics overlay" accessory={<Switch label="Statistics overlay" checked={prefs.showStats} onChange={(showStats) => updatePrefs({ showStats })} />} /><Row icon="pointer" title="Remote cursor" accessory={<Switch label="Remote cursor" checked={prefs.showLocalCursor} onChange={(showLocalCursor) => updatePrefs({ showLocalCursor })} />} /><Row icon="collapse" title="Commandes masquées après 5 secondes" value="Activé" /><Row icon="fullscreen" title="Fullscreen" onClick={() => void toggleFullscreen()} chevron /></Group>
    <Group title="Connection"><Row icon="shield" title="Encryption" value="DTLS + E2EE" /><Row icon="network" title="Path" value={state.stats.candidateType ?? 'Local'} /><Row icon="clock" title="Session" value={state.session?.expiresAt ? new Date(state.session.expiresAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : 'Active'} /></Group>
    <Button variant="danger" icon="power" block onClick={() => { connection.disconnect(); setScreen('home'); }}>Disconnect</Button>
  </>;
}

function Stats({ state }: { state: ReturnType<typeof connection.getSnapshot> }) {
  const stats = state.stats;
  return <aside className="stats-overlay" aria-label="Connection statistics"><div className="stats-title"><span className="vp-dot" data-tone="green" /> Live</div><dl><dt>Ping</dt><dd>{formatMs(stats.pingMs)}</dd><dt>Input</dt><dd>{formatMs(stats.inputLatencyMs)}</dd><dt>Video</dt><dd>{formatMs(stats.videoLatencyMs)}</dd><dt>Decoded</dt><dd>{stats.decodedFps?.toFixed(0) ?? '—'} FPS</dd><dt>Captured</dt><dd>{stats.agent?.captureFps?.toFixed(0) ?? '—'} FPS</dd><dt>Sent</dt><dd>{stats.agent?.sentFps?.toFixed(0) ?? '—'} FPS</dd><dt>Bitrate</dt><dd>{formatBitrate(stats.bitrateKbps ?? undefined)}</dd><dt>Loss</dt><dd>{stats.packetLossPct == null ? '—' : `${stats.packetLossPct.toFixed(2)}%`}</dd><dt>Jitter</dt><dd>{formatMs(stats.jitterMs)}</dd><dt>Codec</dt><dd>{stats.codec?.toUpperCase() ?? '—'}</dd></dl></aside>;
}

function Metric({ label, value }: { label: string; value: string }) { return <div className="metric"><b>{value}</b><span>{label}</span></div>; }

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
