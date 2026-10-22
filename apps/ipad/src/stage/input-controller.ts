/**
 * Touch, Apple Pencil, trackpad and mouse → Windows input.
 *
 * Direct Touch: tap = click, drag = drag, long press = right click, two fingers = scroll,
 *               two-finger tap = right click, pinch = zoom, three-finger tap = hide interface.
 * Trackpad:     one finger moves the cursor, tap = click, double-tap-and-hold = drag,
 *               two fingers = scroll (with momentum), two-finger tap = right click,
 *               three-finger swipe = configurable action.
 * Pencil:       Apple Pencil is a Windows Ink pen (pressure, tilt, hover); fingers navigate.
 *
 * Every event is sent the moment it happens.
 */
import { InputEncoder, MouseButton, PenButtons, PenPhase, VK, type MouseButtonId } from '@vision-pro/protocol';
import type { ControlMode, Preferences } from '../lib/storage.ts';

export interface InputSink {
  reliable(buffer: ArrayBuffer): void;
  realtime(buffer: ArrayBuffer): void;
}

export interface InputCallbacks {
  onToggleInterface(): void;
  onFeedback(kind: 'tap' | 'right' | 'drag', clientX: number, clientY: number): void;
  onUndo(): void;
  onRedo(): void;
  onActivity(): void;
  modifiersForClick(): void;
  onInputSource(source: InputSource): void;
}

/** The physical source reported by iPadOS Pointer Events, independent from the chosen control mode. */
export type InputSource = 'fingers' | 'pencil' | 'pointer';
export type ThreeDNavigation = 'none' | 'blender' | 'maya' | 'zbrush' | 'houdini';

interface TrackedPointer {
  id: number;
  x0: number;
  y0: number;
  x: number;
  y: number;
  t0: number;
}

type Gesture =
  | 'none'
  | 'tap'
  | 'drag'
  | 'long-press'
  | 'two'
  | 'scroll'
  | 'pinch'
  | 'orbit-3d'
  | 'pan-3d'
  | 'three'
  | 'three-done'
  | 'track'
  | 'track-drag'
  /** Fingers resting on the tablet while the Pencil draws: ignored. */
  | 'rest';

const SLOP = 10;
const LONG_PRESS_MS = 480;
const TAP_MS = 280;
const PALM_WINDOW_MS = 600;

type Settings = Pick<
  Preferences,
  | 'trackingSpeed'
  | 'scrollSpeed'
  | 'naturalScrolling'
  | 'tapToClick'
  | 'threeFingerSwipe'
  | 'pencilWindowsInk'
  | 'pencilHover'
  | 'pencilPressureCurve'
  | 'pencilFingers'
  | 'twoFingerTapUndo'
>;

export class InputController {
  readonly encoder = new InputEncoder();
  private pointers = new Map<number, TrackedPointer>();
  private gesture: Gesture = 'none';
  private longPressTimer = 0;
  private lastTap = { t: 0, x: 0, y: 0 };
  private maxPointers = 0;
  private gestureStart = 0;
  private twoStart = { dist: 0, cx: 0, cy: 0 };
  private lastCentroid = { x: 0, y: 0 };
  private lastDist = 0;
  private scrollVelocity = { x: 0, y: 0, t: 0 };
  private momentumFrame = 0;
  private penInContact = false;
  private penHovering = false;
  private penSeenAt = 0;
  private dragArmed = false;
  private ctrlForPinch = false;
  mode: ControlMode = 'direct';
  eraser = false;
  barrel = false;
  enabled = true;

  constructor(
    private readonly surface: HTMLElement,
    private readonly contentRect: () => DOMRect | null,
    private readonly sink: InputSink,
    private settings: Settings,
    private readonly callbacks: InputCallbacks,
    /** Native viewport navigation scheme of the foreground creative application. */
    private readonly threeDNavigation: ThreeDNavigation = 'none',
  ) {}

  setSettings(settings: Settings): void {
    this.settings = settings;
  }

  attach(): () => void {
    const s = this.surface;
    const opts = { passive: false } as AddEventListenerOptions;
    const down = (e: PointerEvent) => this.onDown(e);
    const move = (e: PointerEvent) => this.onMove(e);
    const up = (e: PointerEvent) => this.onUp(e, false);
    const cancel = (e: PointerEvent) => this.onUp(e, true);
    const lostCapture = (e: PointerEvent) => this.onUp(e, true);
    const leave = (e: PointerEvent) => this.onLeave(e);
    const wheel = (e: WheelEvent) => this.onWheel(e);
    const prevent = (e: Event) => e.preventDefault();
    s.addEventListener('pointerdown', down, opts);
    s.addEventListener('pointermove', move, opts);
    s.addEventListener('pointerup', up, opts);
    s.addEventListener('pointercancel', cancel, opts);
    s.addEventListener('lostpointercapture', lostCapture, opts);
    s.addEventListener('pointerleave', leave, opts);
    s.addEventListener('wheel', wheel, opts);
    s.addEventListener('contextmenu', prevent);
    s.addEventListener('gesturestart', prevent);
    s.addEventListener('gesturechange', prevent);
    s.addEventListener('touchstart', prevent, opts);
    s.addEventListener('touchmove', prevent, opts);
    return () => {
      s.removeEventListener('pointerdown', down);
      s.removeEventListener('pointermove', move);
      s.removeEventListener('pointerup', up);
      s.removeEventListener('pointercancel', cancel);
      s.removeEventListener('lostpointercapture', lostCapture);
      s.removeEventListener('pointerleave', leave);
      s.removeEventListener('wheel', wheel);
      s.removeEventListener('contextmenu', prevent);
      s.removeEventListener('gesturestart', prevent);
      s.removeEventListener('gesturechange', prevent);
      s.removeEventListener('touchstart', prevent);
      s.removeEventListener('touchmove', prevent);
      this.releaseAll();
    };
  }

  releaseAll(): void {
    window.clearTimeout(this.longPressTimer);
    cancelAnimationFrame(this.momentumFrame);
    if (this.gesture === 'drag' || this.gesture === 'track-drag') this.sink.reliable(this.encoder.button(MouseButton.Left, false, null, null));
    this.end3DGesture();
    if (this.ctrlForPinch) this.sink.reliable(this.encoder.key(VK.Ctrl, false));
    this.ctrlForPinch = false;
    if (this.penInContact) this.penPhase(PenPhase.Cancel, 0.5, 0.5, 0, 0, 0);
    this.pointers.clear();
    this.gesture = 'none';
    this.penInContact = false;
    this.penHovering = false;
    this.sink.reliable(this.encoder.releaseAll());
  }

  // ---------------------------------------------------------------------------
  // Coordinates
  // ---------------------------------------------------------------------------

  private normalize(clientX: number, clientY: number): { x: number; y: number; inside: boolean } {
    const rect = this.contentRect();
    if (!rect || rect.width < 1 || rect.height < 1) return { x: 0.5, y: 0.5, inside: false };
    const x = (clientX - rect.left) / rect.width;
    const y = (clientY - rect.top) / rect.height;
    return { x: Math.min(1, Math.max(0, x)), y: Math.min(1, Math.max(0, y)), inside: x >= 0 && x <= 1 && y >= 0 && y <= 1 };
  }

  private centroid(): { x: number; y: number } {
    let x = 0;
    let y = 0;
    for (const p of this.pointers.values()) {
      x += p.x;
      y += p.y;
    }
    const n = Math.max(1, this.pointers.size);
    return { x: x / n, y: y / n };
  }

  private spread(): number {
    const list = [...this.pointers.values()];
    if (list.length < 2) return 0;
    return Math.hypot(list[0]!.x - list[1]!.x, list[0]!.y - list[1]!.y);
  }

  private navigationButton: MouseButtonId | null = null;
  private navigationKeys: number[] = [];

  private navigationBinding(kind: 'orbit' | 'pan'): { button: MouseButtonId; keys: number[] } {
    switch (this.threeDNavigation) {
      case 'maya': return { button: kind === 'orbit' ? MouseButton.Left : MouseButton.Middle, keys: [VK.Alt] };
      case 'houdini': return { button: kind === 'orbit' ? MouseButton.Left : MouseButton.Middle, keys: [VK.Space] };
      case 'zbrush': return { button: MouseButton.Left, keys: kind === 'orbit' ? [] : [VK.Alt] };
      case 'blender': return { button: MouseButton.Middle, keys: kind === 'orbit' ? [] : [VK.Shift] };
      default: return { button: MouseButton.Middle, keys: [] };
    }
  }

  private begin3DGesture(kind: 'orbit' | 'pan', centroid: { x: number; y: number }): void {
    this.end3DGesture();
    const n = this.normalize(centroid.x, centroid.y);
    this.sink.reliable(this.encoder.moveAbs(n.x, n.y));
    const binding = this.navigationBinding(kind);
    for (const key of binding.keys) this.sink.reliable(this.encoder.key(key, true));
    this.navigationKeys = binding.keys;
    this.navigationButton = binding.button;
    this.sink.reliable(this.encoder.button(binding.button, true, n.x, n.y));
  }

  private end3DGesture(): void {
    if (this.navigationButton !== null) this.sink.reliable(this.encoder.button(this.navigationButton, false, null, null));
    for (const key of [...this.navigationKeys].reverse()) this.sink.reliable(this.encoder.key(key, false));
    this.navigationButton = null;
    this.navigationKeys = [];
  }

  private canUseThreeFingerNavigation(): boolean {
    return this.threeDNavigation !== 'none' && (this.mode !== 'pencil' || this.settings.pencilFingers === 'touch');
  }

  // ---------------------------------------------------------------------------
  // Pointer routing
  // ---------------------------------------------------------------------------

  private onDown(e: PointerEvent): void {
    e.preventDefault();
    if (!this.enabled) return;
    this.callbacks.onActivity();
    cancelAnimationFrame(this.momentumFrame);
    try {
      this.surface.setPointerCapture(e.pointerId);
    } catch {
      /* pointer already gone */
    }
    if (e.pointerType === 'pen') { this.callbacks.onInputSource('pencil'); return this.penDown(e); }
    if (e.pointerType === 'mouse') { this.callbacks.onInputSource('pointer'); return this.mouseButton(e, true); }
    this.callbacks.onInputSource('fingers');
    if (Date.now() - this.penSeenAt < PALM_WINDOW_MS || this.penInContact) return; // palm rejection
    if (this.mode === 'pencil' && this.settings.pencilFingers === 'ignore') {
      this.pointers.set(e.pointerId, { id: e.pointerId, x0: e.clientX, y0: e.clientY, x: e.clientX, y: e.clientY, t0: performance.now() });
      this.maxPointers = Math.max(this.maxPointers, this.pointers.size);
      if (this.pointers.size === 3) this.gesture = 'three';
      return;
    }
    this.pointers.set(e.pointerId, { id: e.pointerId, x0: e.clientX, y0: e.clientY, x: e.clientX, y: e.clientY, t0: performance.now() });
    const count = this.pointers.size;
    if (count === 1) {
      this.maxPointers = 1;
      this.gestureStart = performance.now();
    }
    this.maxPointers = Math.max(this.maxPointers, count);

    const touchMode = this.mode === 'pencil' && this.settings.pencilFingers === 'touch' ? 'direct' : this.mode;
    if (count === 1) {
      if (touchMode === 'direct') {
        this.gesture = 'tap';
        window.clearTimeout(this.longPressTimer);
        this.longPressTimer = window.setTimeout(() => this.longPress(e.pointerId), LONG_PRESS_MS);
      } else if (touchMode === 'trackpad') {
        const now = performance.now();
        this.dragArmed = now - this.lastTap.t < 320 && Math.hypot(e.clientX - this.lastTap.x, e.clientY - this.lastTap.y) < 40;
        this.gesture = 'track';
      } else {
        this.gesture = 'rest';
      }
    } else if (count === 2) {
      window.clearTimeout(this.longPressTimer);
      if (this.gesture === 'drag' || this.gesture === 'track-drag') {
        const p = [...this.pointers.values()][0]!;
        const n = this.normalize(p.x, p.y);
        this.sink.reliable(this.encoder.button(MouseButton.Left, false, this.gesture === 'drag' ? n.x : null, this.gesture === 'drag' ? n.y : null));
      }
      this.gesture = 'two';
      const c = this.centroid();
      this.twoStart = { dist: this.spread(), cx: c.x, cy: c.y };
      this.lastCentroid = c;
      this.lastDist = this.twoStart.dist;
    } else if (count === 3) {
      if (this.ctrlForPinch) {
        this.sink.reliable(this.encoder.key(VK.Ctrl, false));
        this.ctrlForPinch = false;
      }
      this.end3DGesture();
      this.gesture = 'three';
      this.lastCentroid = this.centroid();
    }
  }

  private onMove(e: PointerEvent): void {
    if (!this.enabled) return;
    if (e.pointerType === 'pen') { this.callbacks.onInputSource('pencil'); return this.penMove(e); }
    if (e.pointerType === 'mouse') { this.callbacks.onInputSource('pointer'); return this.mouseMove(e); }
    const p = this.pointers.get(e.pointerId);
    if (!p) return;
    e.preventDefault();
    const prevX = p.x;
    const prevY = p.y;
    p.x = e.clientX;
    p.y = e.clientY;
    const moved = Math.hypot(p.x - p.x0, p.y - p.y0);

    switch (this.gesture) {
      case 'tap':
        if (moved > SLOP) {
          window.clearTimeout(this.longPressTimer);
          this.gesture = 'drag';
          const start = this.normalize(p.x0, p.y0);
          this.sink.reliable(this.encoder.moveAbs(start.x, start.y));
          this.callbacks.modifiersForClick();
          this.sink.reliable(this.encoder.button(MouseButton.Left, true, start.x, start.y));
          const now = this.normalize(p.x, p.y);
          this.sink.reliable(this.encoder.moveAbs(now.x, now.y));
          this.callbacks.onFeedback('drag', p.x0, p.y0);
        }
        return;
      case 'drag': {
        for (const sample of coalesced(e)) {
          const n = this.normalize(sample.clientX, sample.clientY);
          // A newer position is always more useful than an old one. Keep drag
          // start/end reliable, but never queue the intermediate cursor path.
          this.sink.realtime(this.encoder.moveAbs(n.x, n.y));
        }
        return;
      }
      case 'track':
      case 'track-drag': {
        const dx = p.x - prevX;
        const dy = p.y - prevY;
        if (this.gesture === 'track' && this.dragArmed && moved > 4) {
          this.gesture = 'track-drag';
          this.callbacks.modifiersForClick();
          this.sink.reliable(this.encoder.button(MouseButton.Left, true, null, null));
        }
        this.relativeMove(dx, dy, e.timeStamp);
        return;
      }
      case 'rest':
        return;
      case 'two':
      case 'scroll':
      case 'pinch':
      case 'orbit-3d':
      case 'pan-3d':
        this.twoFingerMove();
        return;
      case 'three': {
        const c = this.centroid();
        const dx = c.x - this.lastCentroid.x;
        const dy = c.y - this.lastCentroid.y;
        if (this.canUseThreeFingerNavigation() && Math.hypot(dx, dy) > SLOP) {
          this.gesture = 'orbit-3d';
          this.begin3DGesture('orbit', c);
          this.lastCentroid = c;
          return;
        }
        if (Math.hypot(dx, dy) > 70) {
          this.threeFingerSwipe(dx, dy);
          this.gesture = 'three-done';
        }
        return;
      }
    }
  }

  private twoFingerMove(): void {
    const c = this.centroid();
    const dist = this.spread();
    if (this.gesture === 'two') {
      const pinchDelta = Math.abs(dist - this.twoStart.dist);
      const panDelta = Math.hypot(c.x - this.twoStart.cx, c.y - this.twoStart.cy);
      // Two fingers navigate the application under the Pencil: pinch zooms it (Ctrl+wheel, or
      // the 3D viewport's own zoom), a two-finger drag scrolls or pans it.
      if (pinchDelta > 18 && pinchDelta > panDelta * 0.6) {
        this.gesture = 'pinch';
        if (this.threeDNavigation === 'none') {
          this.sink.reliable(this.encoder.key(VK.Ctrl, true));
          this.ctrlForPinch = true;
        }
      } else if (panDelta > 8) {
        this.gesture = this.threeDNavigation !== 'none' ? 'pan-3d' : 'scroll';
        if (this.gesture === 'pan-3d') this.begin3DGesture('pan', c);
      } else {
        return;
      }
      this.lastCentroid = c;
      this.lastDist = dist;
      return;
    }
    const dx = c.x - this.lastCentroid.x;
    const dy = c.y - this.lastCentroid.y;
    if (this.gesture === 'pinch') {
      const ratio = dist / Math.max(1, this.lastDist);
      if (Math.abs(ratio - 1) > 0.04) {
        const n = this.normalize(c.x, c.y);
        this.sink.reliable(this.encoder.wheel(0, ratio > 1 ? -120 : 120, n.x, n.y, false));
        this.lastDist = dist;
      }
    } else if (this.gesture === 'scroll') {
      this.scroll(dx, dy, c.x, c.y);
    } else if (this.gesture === 'orbit-3d' || this.gesture === 'pan-3d') {
      const n = this.normalize(c.x, c.y);
      this.sink.realtime(this.encoder.moveAbs(n.x, n.y));
    }
    this.lastCentroid = c;
  }

  private scroll(dx: number, dy: number, cx: number | null, cy: number | null): void {
    const factor = 2.2 * this.settings.scrollSpeed * (this.settings.naturalScrolling ? 1 : -1);
    const n = cx !== null && cy !== null && this.mode === 'direct' ? this.normalize(cx, cy) : null;
    this.sink.reliable(this.encoder.wheel(-dx * factor, -dy * factor, n?.x ?? null, n?.y ?? null, true));
    const now = performance.now();
    const dt = Math.max(1, now - this.scrollVelocity.t);
    this.scrollVelocity = { x: dx / dt, y: dy / dt, t: now };
  }

  private startMomentum(): void {
    let vx = this.scrollVelocity.x * 16;
    let vy = this.scrollVelocity.y * 16;
    if (performance.now() - this.scrollVelocity.t > 80 || Math.hypot(vx, vy) < 4) return;
    const step = () => {
      vx *= 0.93;
      vy *= 0.93;
      if (Math.hypot(vx, vy) < 0.6) return;
      const factor = 2.2 * this.settings.scrollSpeed * (this.settings.naturalScrolling ? 1 : -1);
      this.sink.reliable(this.encoder.wheel(-vx * factor, -vy * factor, null, null, true));
      this.momentumFrame = requestAnimationFrame(step);
    };
    this.momentumFrame = requestAnimationFrame(step);
  }

  private relativeMove(dx: number, dy: number, timeStamp: number): void {
    const rect = this.contentRect();
    if (!rect) return;
    const speed = Math.hypot(dx, dy) / Math.max(1, timeStamp - (this.lastMoveAt || timeStamp - 16));
    this.lastMoveAt = timeStamp;
    const acceleration = 1 + Math.min(2.5, speed * 1.4);
    const gain = this.settings.trackingSpeed * acceleration;
    const nx = (dx * gain) / Math.max(1, rect.width);
    const ny = (dy * gain) / Math.max(1, rect.height);
    this.sink.realtime(this.encoder.moveRel(nx, ny));
  }
  private lastMoveAt = 0;

  private onUp(e: PointerEvent, cancelled: boolean): void {
    if (e.pointerType === 'pen') return this.penUp(e, cancelled);
    if (e.pointerType === 'mouse') return this.mouseButton(e, false);
    const p = this.pointers.get(e.pointerId);
    if (!p) return;
    e.preventDefault();
    if (cancelled) {
      // iPadOS cancels pointer sequences when a system gesture, a palm or a
      // focus change takes over. Keeping the remaining finger as a drag makes
      // the remote cursor jump unexpectedly, so cancel the entire gesture.
      this.cancelTouchGesture();
      return;
    }
    // pointerup may be the first event carrying the final coordinate.  Move
    // there reliably before releasing a direct drag: intermediate moves travel
    // on the latest-wins channel and may legitimately have been dropped.
    p.x = e.clientX;
    p.y = e.clientY;
    const elapsed = performance.now() - this.gestureStart;
    const touchMode = this.mode === 'pencil' && this.settings.pencilFingers === 'touch' ? 'direct' : this.mode;

    if (this.pointers.size === 1) {
      switch (this.gesture) {
        case 'tap':
          window.clearTimeout(this.longPressTimer);
          if (this.maxPointers === 1 && touchMode === 'direct') this.click(p.x, p.y);
          break;
        case 'drag': {
          const n = this.normalize(p.x, p.y);
          this.sink.reliable(this.encoder.moveAbs(n.x, n.y));
          this.sink.reliable(this.encoder.button(MouseButton.Left, false, n.x, n.y));
          break;
        }
        case 'track':
          if (this.maxPointers === 1 && elapsed < TAP_MS && Math.hypot(p.x - p.x0, p.y - p.y0) < SLOP && this.settings.tapToClick) {
            this.callbacks.modifiersForClick();
            this.sink.reliable(this.encoder.button(MouseButton.Left, true, null, null));
            this.sink.reliable(this.encoder.button(MouseButton.Left, false, null, null));
            this.lastTap = { t: performance.now(), x: p.x, y: p.y };
            this.callbacks.onFeedback('tap', p.x, p.y);
          }
          break;
        case 'track-drag':
          this.sink.reliable(this.encoder.button(MouseButton.Left, false, null, null));
          break;
        case 'two':
          if (this.maxPointers === 2 && elapsed < TAP_MS + 80) this.twoFingerTap();
          break;
        case 'scroll':
          this.startMomentum();
          break;
        case 'three':
          if (this.maxPointers === 3 && elapsed < TAP_MS + 120) this.threeFingerTap();
          break;
      }
    }
    this.pointers.delete(e.pointerId);
    if (this.pointers.size === 0) {
      if (this.ctrlForPinch) {
        this.sink.reliable(this.encoder.key(VK.Ctrl, false));
        this.ctrlForPinch = false;
      }
      this.end3DGesture();
      this.gesture = 'none';
      this.dragArmed = false;
    } else if (this.gesture === 'two' && this.pointers.size === 1) {
      // Wait for the other finger: a two-finger tap is resolved when the last finger lifts.
    } else if (this.gesture === 'pinch' || this.gesture === 'scroll') {
      // Once decided, a two-finger gesture stays until every finger lifts.
    }
  }

  private cancelTouchGesture(): void {
    window.clearTimeout(this.longPressTimer);
    cancelAnimationFrame(this.momentumFrame);
    if (this.gesture === 'drag' || this.gesture === 'track-drag') {
      this.sink.reliable(this.encoder.button(MouseButton.Left, false, null, null));
    }
    if (this.ctrlForPinch) {
      this.sink.reliable(this.encoder.key(VK.Ctrl, false));
      this.ctrlForPinch = false;
    }
    this.end3DGesture();
    this.pointers.clear();
    this.gesture = 'none';
    this.dragArmed = false;
    this.maxPointers = 0;
    this.scrollVelocity = { x: 0, y: 0, t: 0 };
  }

  private onLeave(e: PointerEvent): void {
    if (e.pointerType === 'pen' && this.penHovering && !this.penInContact) {
      const n = this.normalize(e.clientX, e.clientY);
      this.penPhase(PenPhase.Leave, n.x, n.y, 0, 0, 0);
      this.penHovering = false;
    }
  }

  private click(clientX: number, clientY: number): void {
    const n = this.normalize(clientX, clientY);
    if (!n.inside) return;
    this.callbacks.modifiersForClick();
    this.sink.reliable(this.encoder.button(MouseButton.Left, true, n.x, n.y));
    this.sink.reliable(this.encoder.button(MouseButton.Left, false, n.x, n.y));
    this.callbacks.onFeedback('tap', clientX, clientY);
  }

  private longPress(pointerId: number): void {
    const p = this.pointers.get(pointerId);
    if (!p || this.gesture !== 'tap' || this.pointers.size !== 1) return;
    this.gesture = 'long-press';
    const n = this.normalize(p.x, p.y);
    this.callbacks.modifiersForClick();
    this.sink.reliable(this.encoder.button(MouseButton.Right, true, n.x, n.y));
    this.sink.reliable(this.encoder.button(MouseButton.Right, false, n.x, n.y));
    this.callbacks.onFeedback('right', p.x, p.y);
  }

  private twoFingerTap(): void {
    if (this.mode === 'pencil' && this.settings.pencilFingers !== 'touch') {
      if (this.settings.twoFingerTapUndo) this.callbacks.onUndo();
      return;
    }
    const c = this.centroid();
    const n = this.normalize(c.x, c.y);
    const at = this.mode === 'direct';
    this.callbacks.modifiersForClick();
    this.sink.reliable(this.encoder.button(MouseButton.Right, true, at ? n.x : null, at ? n.y : null));
    this.sink.reliable(this.encoder.button(MouseButton.Right, false, at ? n.x : null, at ? n.y : null));
    this.callbacks.onFeedback('right', c.x, c.y);
  }

  private threeFingerTap(): void {
    if (this.mode === 'pencil' && this.settings.pencilFingers !== 'touch' && this.settings.twoFingerTapUndo) {
      this.callbacks.onRedo();
      return;
    }
    this.callbacks.onToggleInterface();
  }

  private threeFingerSwipe(dx: number, dy: number): void {
    const horizontal = Math.abs(dx) > Math.abs(dy);
    const action = this.settings.threeFingerSwipe;
    if (action === 'none') return;
    let keys: number[] | null = null;
    if (action === 'task-view') keys = horizontal ? [VK.Alt, VK.Tab] : dy < 0 ? [VK.Win, VK.Tab] : [VK.Win, VK.D];
    if (action === 'show-desktop') keys = [VK.Win, VK.D];
    if (action === 'switch-app') keys = [VK.Alt, VK.Tab];
    if (action === 'desktops') keys = horizontal ? [VK.Ctrl, VK.Win, dx < 0 ? VK.Right : VK.Left] : [VK.Win, VK.Tab];
    if (keys) this.sink.reliable(this.encoder.combo(keys));
  }

  // ---------------------------------------------------------------------------
  // Apple Pencil
  // ---------------------------------------------------------------------------

  private pressure(raw: number): number {
    const p = Math.min(1, Math.max(0, raw));
    switch (this.settings.pencilPressureCurve) {
      case 'soft':
        return Math.pow(p, 0.6);
      case 'firm':
        return Math.pow(p, 1.6);
      default:
        return p;
    }
  }

  private tilt(e: PointerEvent): { tx: number; ty: number; twist: number } {
    let tx = e.tiltX || 0;
    let ty = e.tiltY || 0;
    const extended = e as PointerEvent & { altitudeAngle?: number; azimuthAngle?: number };
    if (tx === 0 && ty === 0 && typeof extended.altitudeAngle === 'number' && typeof extended.azimuthAngle === 'number') {
      const altitude = extended.altitudeAngle;
      const azimuth = extended.azimuthAngle;
      if (altitude < Math.PI / 2 - 0.001) {
        const tanAlt = Math.tan(altitude);
        tx = (Math.atan(Math.cos(azimuth) / tanAlt) * 180) / Math.PI;
        ty = (Math.atan(Math.sin(azimuth) / tanAlt) * 180) / Math.PI;
      }
    }
    return { tx, ty, twist: e.twist || 0 };
  }

  private penButtons(): number {
    return (this.barrel ? PenButtons.Barrel : 0) | (this.eraser ? PenButtons.Eraser : 0);
  }

  private penPhase(phase: number, x: number, y: number, pressure: number, tiltX: number, tiltY: number, twist = 0, realtime = false): void {
    const buffer = this.encoder.pen({ phase, x, y, pressure, tiltX, tiltY, twist, buttons: this.penButtons() });
    if (realtime) this.sink.realtime(buffer);
    else this.sink.reliable(buffer);
  }

  private penDown(e: PointerEvent): void {
    this.penSeenAt = Date.now();
    // A pen landing while fingers are down cancels any finger gesture (palm on screen).
    if (this.pointers.size > 0) {
      if (this.gesture === 'drag') this.sink.reliable(this.encoder.button(MouseButton.Left, false, null, null));
      this.pointers.clear();
      this.gesture = 'none';
      window.clearTimeout(this.longPressTimer);
    }
    const n = this.normalize(e.clientX, e.clientY);
    if (!this.settings.pencilWindowsInk) {
      this.sink.reliable(this.encoder.button(this.barrel ? MouseButton.Right : MouseButton.Left, true, n.x, n.y));
      this.penInContact = true;
      return;
    }
    const { tx, ty, twist } = this.tilt(e);
    this.penPhase(PenPhase.Down, n.x, n.y, this.pressure(e.pressure || 0.5), tx, ty, twist);
    this.penInContact = true;
    this.penHovering = true;
  }

  private penMove(e: PointerEvent): void {
    e.preventDefault();
    this.penSeenAt = Date.now();
    const contact = this.penInContact && (e.buttons & 1) === 1;
    if (!this.settings.pencilWindowsInk) {
      for (const sample of coalesced(e)) {
        const n = this.normalize(sample.clientX, sample.clientY);
        if (contact) this.sink.realtime(this.encoder.moveAbs(n.x, n.y));
        else if (this.settings.pencilHover) this.sink.realtime(this.encoder.moveAbs(n.x, n.y));
      }
      return;
    }
    if (!contact && !this.settings.pencilHover) return;
    for (const sample of coalesced(e)) {
      const n = this.normalize(sample.clientX, sample.clientY);
      const { tx, ty, twist } = this.tilt(sample);
      if (contact) {
        this.penPhase(PenPhase.Move, n.x, n.y, this.pressure(sample.pressure), tx, ty, twist);
      } else {
        this.penPhase(PenPhase.Hover, n.x, n.y, 0, tx, ty, twist, true);
        this.penHovering = true;
      }
    }
  }

  private penUp(e: PointerEvent, cancelled: boolean): void {
    this.penSeenAt = Date.now();
    if (!this.penInContact) return;
    const n = this.normalize(e.clientX, e.clientY);
    if (!this.settings.pencilWindowsInk) {
      this.sink.reliable(this.encoder.button(this.barrel ? MouseButton.Right : MouseButton.Left, false, n.x, n.y));
    } else {
      const { tx, ty, twist } = this.tilt(e);
      this.penPhase(cancelled ? PenPhase.Cancel : PenPhase.Up, n.x, n.y, 0, tx, ty, twist);
    }
    this.penInContact = false;
  }

  // ---------------------------------------------------------------------------
  // Mouse & trackpad connected to the iPad
  // ---------------------------------------------------------------------------

  private mouseButton(e: PointerEvent, down: boolean): void {
    e.preventDefault();
    const button = e.button === 2 ? MouseButton.Right : e.button === 1 ? MouseButton.Middle : e.button === 3 ? MouseButton.Back : e.button === 4 ? MouseButton.Forward : MouseButton.Left;
    const n = this.normalize(e.clientX, e.clientY);
    if (down) this.callbacks.modifiersForClick();
    this.sink.reliable(this.encoder.button(button, down, n.x, n.y));
  }

  private mouseMove(e: PointerEvent): void {
    this.callbacks.onInputSource('pointer');
    for (const sample of coalesced(e)) {
      const n = this.normalize(sample.clientX, sample.clientY);
      this.sink.realtime(this.encoder.moveAbs(n.x, n.y));
    }
  }

  private onWheel(e: WheelEvent): void {
    e.preventDefault();
    if (!this.enabled) return;
    this.callbacks.onActivity();
    const scale = e.deltaMode === 1 ? 40 : e.deltaMode === 2 ? 400 : 1;
    const n = this.normalize(e.clientX, e.clientY);
    this.sink.reliable(this.encoder.wheel(e.deltaX * scale * 1.2, e.deltaY * scale * 1.2, n.x, n.y, true));
  }
}

function coalesced(e: PointerEvent): PointerEvent[] {
  const list = typeof e.getCoalescedEvents === 'function' ? e.getCoalescedEvents() : [];
  return list.length > 0 ? list : [e];
}
