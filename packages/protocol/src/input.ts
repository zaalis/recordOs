/**
 * Binary input protocol (iPad → Mac). Little-endian, fixed layouts, 4-byte header:
 *   u8 kind | u8 flags | u16 seq
 * Coordinates are normalized to the streamed display: 0.0 = left/top, 1.0 = right/bottom.
 * The agent validates every field (length, finiteness, ranges, key allowlist) before injection.
 * Mirror: agent/crates/vp-agent/src/input.rs
 */
import { LIMITS } from './constants.ts';

export const InputKind = {
  MoveAbs: 0x01,
  MoveRel: 0x02,
  Button: 0x03,
  Wheel: 0x04,
  Key: 0x05,
  Text: 0x06,
  Pen: 0x07,
  Touch: 0x08,
  Combo: 0x09,
  ReleaseAll: 0x0a,
  /** Pinch forwarded as a native macOS magnify gesture. */
  Magnify: 0x0b,
  /** Agent → client, realtime channel. */
  Cursor: 0x40,
} as const;

export const MouseButton = { Left: 0, Right: 1, Middle: 2, Back: 3, Forward: 4 } as const;
export type MouseButtonId = (typeof MouseButton)[keyof typeof MouseButton];

export const PenPhase = { Hover: 0, Down: 1, Move: 2, Up: 3, Leave: 4, Cancel: 5 } as const;
export const TouchPhase = { Down: 1, Move: 2, Up: 3, Cancel: 5 } as const;

export const PenButtons = { Barrel: 1, Eraser: 2 } as const;

export const GesturePhase = { Begin: 0, Change: 1, End: 2 } as const;

export const WheelFlags = { Precise: 1 } as const;

export interface PenSample {
  phase: number;
  x: number;
  y: number;
  pressure: number;
  tiltX: number;
  tiltY: number;
  twist: number;
  buttons: number;
}

export interface TouchContact {
  id: number;
  phase: number;
  x: number;
  y: number;
  pressure: number;
}

const clamp01 = (value: number) => (Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 0);

export class InputEncoder {
  private seq = 0;

  private header(view: DataView, kind: number, flags = 0): void {
    view.setUint8(0, kind);
    view.setUint8(1, flags);
    view.setUint16(2, this.seq, true);
    this.seq = (this.seq + 1) & 0xffff;
  }

  moveAbs(x: number, y: number): ArrayBuffer {
    const buf = new ArrayBuffer(12);
    const view = new DataView(buf);
    this.header(view, InputKind.MoveAbs);
    view.setFloat32(4, clamp01(x), true);
    view.setFloat32(8, clamp01(y), true);
    return buf;
  }

  /** Relative move in normalized display units (1.0 = full display width/height). */
  moveRel(dx: number, dy: number): ArrayBuffer {
    const buf = new ArrayBuffer(12);
    const view = new DataView(buf);
    this.header(view, InputKind.MoveRel);
    view.setFloat32(4, Math.max(-1, Math.min(1, dx || 0)), true);
    view.setFloat32(8, Math.max(-1, Math.min(1, dy || 0)), true);
    return buf;
  }

  /** Pass `x`/`y` as `null` to click at the current cursor position (trackpad mode). */
  button(button: MouseButtonId, down: boolean, x: number | null, y: number | null): ArrayBuffer {
    const buf = new ArrayBuffer(16);
    const view = new DataView(buf);
    this.header(view, InputKind.Button);
    view.setUint8(4, button);
    view.setUint8(5, down ? 1 : 0);
    view.setFloat32(8, x === null ? NaN : clamp01(x), true);
    view.setFloat32(12, y === null ? NaN : clamp01(y), true);
    return buf;
  }

  /**
   * Wheel deltas in wheel units (120 = one mouse-wheel notch). Positive dy scrolls toward the end
   * of the document. `precise` deltas become pixel scrolling on the Mac, others line scrolling.
   */
  wheel(dx: number, dy: number, x: number | null, y: number | null, precise = true): ArrayBuffer {
    const buf = new ArrayBuffer(20);
    const view = new DataView(buf);
    this.header(view, InputKind.Wheel, precise ? WheelFlags.Precise : 0);
    view.setFloat32(4, Math.max(-12000, Math.min(12000, dx || 0)), true);
    view.setFloat32(8, Math.max(-12000, Math.min(12000, dy || 0)), true);
    view.setFloat32(12, x === null ? NaN : clamp01(x), true);
    view.setFloat32(16, y === null ? NaN : clamp01(y), true);
    return buf;
  }

  key(vk: number, down: boolean): ArrayBuffer {
    const buf = new ArrayBuffer(8);
    const view = new DataView(buf);
    this.header(view, InputKind.Key);
    view.setUint16(4, vk, true);
    view.setUint8(6, down ? 1 : 0);
    return buf;
  }

  text(value: string): ArrayBuffer[] {
    const chunks: ArrayBuffer[] = [];
    for (let offset = 0; offset < value.length; offset += LIMITS.maxTextUnits) {
      const part = value.slice(offset, offset + LIMITS.maxTextUnits);
      const buf = new ArrayBuffer(6 + part.length * 2);
      const view = new DataView(buf);
      this.header(view, InputKind.Text);
      view.setUint16(4, part.length, true);
      for (let i = 0; i < part.length; i++) view.setUint16(6 + i * 2, part.charCodeAt(i), true);
      chunks.push(buf);
    }
    return chunks;
  }

  pen(sample: PenSample): ArrayBuffer {
    const buf = new ArrayBuffer(24);
    const view = new DataView(buf);
    this.header(view, InputKind.Pen);
    view.setUint8(4, sample.phase);
    view.setUint8(5, sample.buttons & 0x03);
    view.setInt8(6, Math.round(Math.max(-90, Math.min(90, sample.tiltX || 0))));
    view.setInt8(7, Math.round(Math.max(-90, Math.min(90, sample.tiltY || 0))));
    view.setFloat32(8, clamp01(sample.x), true);
    view.setFloat32(12, clamp01(sample.y), true);
    view.setFloat32(16, clamp01(sample.pressure), true);
    view.setUint16(20, Math.round(((sample.twist % 360) + 360) % 360), true);
    return buf;
  }

  touch(contacts: TouchContact[]): ArrayBuffer {
    const list = contacts.slice(0, LIMITS.maxTouchContacts);
    const buf = new ArrayBuffer(8 + list.length * 16);
    const view = new DataView(buf);
    this.header(view, InputKind.Touch);
    view.setUint8(4, list.length);
    list.forEach((c, i) => {
      const o = 8 + i * 16;
      view.setUint8(o, c.id & 0xff);
      view.setUint8(o + 1, c.phase);
      view.setFloat32(o + 4, clamp01(c.x), true);
      view.setFloat32(o + 8, clamp01(c.y), true);
      view.setFloat32(o + 12, clamp01(c.pressure), true);
    });
    return buf;
  }

  combo(vks: number[]): ArrayBuffer {
    const keys = vks.slice(0, LIMITS.maxComboKeys);
    const buf = new ArrayBuffer(6 + keys.length * 2);
    const view = new DataView(buf);
    this.header(view, InputKind.Combo);
    view.setUint8(4, keys.length);
    keys.forEach((vk, i) => view.setUint16(6 + i * 2, vk, true));
    return buf;
  }

  /**
   * Magnify gesture. `delta` is incremental (0.05 = 5% larger than the previous event), like
   * NSEvent.magnification. Pass `x`/`y` as `null` to use the current pointer position.
   */
  magnify(phase: number, delta: number, x: number | null, y: number | null): ArrayBuffer {
    const buf = new ArrayBuffer(20);
    const view = new DataView(buf);
    this.header(view, InputKind.Magnify);
    view.setUint8(4, phase);
    view.setFloat32(8, Number.isFinite(delta) ? Math.max(-4, Math.min(4, delta)) : 0, true);
    view.setFloat32(12, x === null ? NaN : clamp01(x), true);
    view.setFloat32(16, y === null ? NaN : clamp01(y), true);
    return buf;
  }

  releaseAll(): ArrayBuffer {
    const buf = new ArrayBuffer(4);
    this.header(new DataView(buf), InputKind.ReleaseAll);
    return buf;
  }
}

export interface CursorUpdate {
  x: number;
  y: number;
  shapeId: number;
  visible: boolean;
}

export function decodeCursor(data: ArrayBuffer): CursorUpdate | null {
  if (data.byteLength !== 20) return null;
  const view = new DataView(data);
  if (view.getUint8(0) !== InputKind.Cursor) return null;
  const x = view.getFloat32(4, true);
  const y = view.getFloat32(8, true);
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  return { x, y, shapeId: view.getUint32(12, true), visible: view.getUint8(16) === 1 };
}
