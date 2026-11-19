import { DOM_CODE_TO_KEY, isKeyName, keyLabel, MODIFIER_KEYS, type KeyName } from '@vision-pro/protocol';

const frenchKeys: Partial<Record<KeyName, string>> = {
  Alt: 'Alt', Ctrl: 'Ctrl', Shift: 'Maj', Win: 'Windows',
  Backspace: 'Retour arrière', Enter: 'Entrée', Esc: 'Échap', Space: 'Espace',
  Delete: 'Suppr', Insert: 'Inser', Home: 'Début', End: 'Fin',
  PageUp: 'Page précédente', PageDown: 'Page suivante', PrintScreen: 'Impr. écran',
  CapsLock: 'Verr. maj.', NumLock: 'Verr. num.', ScrollLock: 'Arrêt défil.',
  MediaPrev: 'Piste précédente', MediaNext: 'Piste suivante', MediaStop: 'Arrêter la lecture',
  MediaPlayPause: 'Lecture / pause', VolumeDown: 'Baisser le volume',
  VolumeUp: 'Monter le volume', VolumeMute: 'Couper le son',
  NumEnter: 'Entrée du pavé numérique', NumAdd: '+ du pavé numérique',
  NumSubtract: '− du pavé numérique', NumMultiply: '× du pavé numérique',
  NumDivide: '÷ du pavé numérique', NumDecimal: 'Séparateur du pavé numérique',
};

export const isShortcutModifier = (key: KeyName) => MODIFIER_KEYS.includes(key);
export const shortcutKeyLabel = (key: KeyName) => frenchKeys[key] ?? keyLabel(key);
export const shortcutLabel = (keys: readonly KeyName[]) => keys.map(shortcutKeyLabel).join(' + ');

/** A shortcut has one primary key and at most four distinct modifiers. */
export function normalizeShortcut(value: unknown): KeyName[] {
  if (!Array.isArray(value)) return ['Ctrl', 'Z'];
  const keys = value.filter((key): key is KeyName => typeof key === 'string' && isKeyName(key));
  const primary = keys.find(key => !isShortcutModifier(key));
  return primary ? [...MODIFIER_KEYS.filter(key => keys.includes(key)), primary] : ['Ctrl', 'Z'];
}

export function shortcutFromKeyboard(event: Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>): KeyName[] | undefined {
  const key = event.key.toUpperCase();
  const primary = /^[A-Z0-9]$/.test(key) && isKeyName(key) && !event.code.startsWith('Numpad')
    ? key : DOM_CODE_TO_KEY[event.code];
  if (!primary || isShortcutModifier(primary)) return undefined;
  return normalizeShortcut([
    event.ctrlKey && 'Ctrl',
    event.altKey && 'Alt',
    event.shiftKey && 'Shift',
    event.metaKey && 'Win',
    primary,
  ]);
}
