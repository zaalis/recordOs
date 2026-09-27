/**
 * Keys addressable from the iPad. Only keys in this table can be injected; the Mac agent enforces
 * the same allowlist and translates every code to the physical macOS key
 * (agent/crates/vp-agent/src/platform/keymap.rs). The numeric values are stable wire identifiers;
 * 0x100 marks the keypad variant of a key (keypad Enter, keypad ÷).
 *
 * Modifiers use Mac names: Cmd ⌘, Option ⌥, Control ⌃, Shift ⇧.
 */

export const EXTENDED = 0x100;

export const VK = {
  Backspace: 0x08,
  Tab: 0x09,
  Enter: 0x0d,
  Shift: 0x10,
  Control: 0x11,
  Option: 0x12,
  Pause: 0x13,
  CapsLock: 0x14,
  Esc: 0x1b,
  Space: 0x20,
  PageUp: 0x21,
  PageDown: 0x22,
  End: 0x23,
  Home: 0x24,
  Left: 0x25,
  Up: 0x26,
  Right: 0x27,
  Down: 0x28,
  PrintScreen: 0x2c,
  Insert: 0x2d,
  Delete: 0x2e,
  '0': 0x30, '1': 0x31, '2': 0x32, '3': 0x33, '4': 0x34, '5': 0x35, '6': 0x36, '7': 0x37, '8': 0x38, '9': 0x39,
  A: 0x41, B: 0x42, C: 0x43, D: 0x44, E: 0x45, F: 0x46, G: 0x47, H: 0x48, I: 0x49, J: 0x4a, K: 0x4b, L: 0x4c, M: 0x4d,
  N: 0x4e, O: 0x4f, P: 0x50, Q: 0x51, R: 0x52, S: 0x53, T: 0x54, U: 0x55, V: 0x56, W: 0x57, X: 0x58, Y: 0x59, Z: 0x5a,
  Cmd: 0x5b,
  Menu: 0x5d,
  Num0: 0x60, Num1: 0x61, Num2: 0x62, Num3: 0x63, Num4: 0x64, Num5: 0x65, Num6: 0x66, Num7: 0x67, Num8: 0x68, Num9: 0x69,
  NumMultiply: 0x6a,
  NumAdd: 0x6b,
  NumSubtract: 0x6d,
  NumDecimal: 0x6e,
  NumDivide: 0x6f | EXTENDED,
  NumEnter: 0x0d | EXTENDED,
  F1: 0x70, F2: 0x71, F3: 0x72, F4: 0x73, F5: 0x74, F6: 0x75, F7: 0x76, F8: 0x77, F9: 0x78, F10: 0x79, F11: 0x7a, F12: 0x7b,
  F13: 0x7c, F14: 0x7d, F15: 0x7e, F16: 0x7f, F17: 0x80, F18: 0x81, F19: 0x82, F20: 0x83, F21: 0x84, F22: 0x85, F23: 0x86, F24: 0x87,
  NumLock: 0x90,
  ScrollLock: 0x91,
  VolumeMute: 0xad,
  VolumeDown: 0xae,
  VolumeUp: 0xaf,
  MediaNext: 0xb0,
  MediaPrev: 0xb1,
  MediaStop: 0xb2,
  MediaPlayPause: 0xb3,
  Semicolon: 0xba,
  Equal: 0xbb,
  Comma: 0xbc,
  Minus: 0xbd,
  Period: 0xbe,
  Slash: 0xbf,
  Backquote: 0xc0,
  BracketLeft: 0xdb,
  Backslash: 0xdc,
  BracketRight: 0xdd,
  Quote: 0xde,
  IntlBackslash: 0xe2,
} as const;

export type KeyName = keyof typeof VK;

export const ALLOWED_VKS: ReadonlySet<number> = new Set(Object.values(VK));

export const MODIFIER_KEYS: readonly KeyName[] = ['Cmd', 'Option', 'Control', 'Shift'];

/** Short labels used on key caps. */
export const KEY_LABEL: Partial<Record<KeyName, string>> = {
  Cmd: '⌘',
  Option: '⌥',
  Control: '⌃',
  Shift: '⇧',
  CapsLock: '⇪',
  Tab: '⇥',
  Esc: 'esc',
  Enter: '↩',
  Backspace: '⌫',
  Delete: '⌦',
  Space: 'Espace',
  Up: '↑',
  Down: '↓',
  Left: '←',
  Right: '→',
  Home: '↖',
  End: '↘',
  PageUp: '⇞',
  PageDown: '⇟',
  Insert: 'Aide',
  Menu: 'Menu',
  BracketLeft: '[',
  BracketRight: ']',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
  Backslash: '\\',
  Backquote: '`',
  Minus: '-',
  Equal: '=',
  NumAdd: 'Num +',
  NumSubtract: 'Num −',
  NumMultiply: 'Num ×',
  NumDivide: 'Num ÷',
  NumDecimal: 'Num .',
  NumEnter: 'Num ↩',
  NumLock: 'Clear',
  PrintScreen: 'F13',
};

export function keyLabel(name: KeyName): string {
  const label = KEY_LABEL[name];
  if (label) return label;
  if (name.startsWith('Num') && name.length === 4) return `Num ${name.slice(3)}`;
  return name;
}

/** Mac-style shortcut label, modifiers in Apple's canonical order: ⌃⌥⇧⌘K. */
export function comboLabel(keys: readonly KeyName[]): string {
  const order: KeyName[] = ['Control', 'Option', 'Shift', 'Cmd'];
  const modifiers = order.filter((key) => keys.includes(key));
  const others = keys.filter((key) => !order.includes(key));
  const glyphs = modifiers.map(keyLabel).join('');
  const rest = others.map(keyLabel).join(' ');
  return glyphs && rest ? `${glyphs}${rest.length > 1 ? ' ' : ''}${rest}` : glyphs || rest;
}

/** KeyboardEvent.code → key name, for hardware keyboards attached to the iPad. */
export const DOM_CODE_TO_KEY: Record<string, KeyName> = (() => {
  const map: Record<string, KeyName> = {
    Backspace: 'Backspace', Tab: 'Tab', Enter: 'Enter', ShiftLeft: 'Shift', ShiftRight: 'Shift',
    ControlLeft: 'Control', ControlRight: 'Control', AltLeft: 'Option', AltRight: 'Option', Pause: 'Pause',
    CapsLock: 'CapsLock', Escape: 'Esc', Space: 'Space', PageUp: 'PageUp', PageDown: 'PageDown',
    End: 'End', Home: 'Home', ArrowLeft: 'Left', ArrowUp: 'Up', ArrowRight: 'Right', ArrowDown: 'Down',
    PrintScreen: 'PrintScreen', Insert: 'Insert', Delete: 'Delete', MetaLeft: 'Cmd', MetaRight: 'Cmd',
    ContextMenu: 'Menu', NumpadMultiply: 'NumMultiply', NumpadAdd: 'NumAdd', NumpadSubtract: 'NumSubtract',
    NumpadDecimal: 'NumDecimal', NumpadDivide: 'NumDivide', NumpadEnter: 'NumEnter', NumLock: 'NumLock',
    ScrollLock: 'ScrollLock', Semicolon: 'Semicolon', Equal: 'Equal', Comma: 'Comma', Minus: 'Minus',
    Period: 'Period', Slash: 'Slash', Backquote: 'Backquote', BracketLeft: 'BracketLeft',
    Backslash: 'Backslash', BracketRight: 'BracketRight', Quote: 'Quote', IntlBackslash: 'IntlBackslash',
  };
  for (let i = 0; i <= 9; i++) {
    map[`Digit${i}`] = String(i) as KeyName;
    map[`Numpad${i}`] = `Num${i}` as KeyName;
  }
  for (let c = 65; c <= 90; c++) {
    const letter = String.fromCharCode(c) as KeyName;
    map[`Key${letter}`] = letter;
  }
  for (let i = 1; i <= 24; i++) map[`F${i}`] = `F${i}` as KeyName;
  return map;
})();

export function isKeyName(value: string): value is KeyName {
  return Object.prototype.hasOwnProperty.call(VK, value);
}
