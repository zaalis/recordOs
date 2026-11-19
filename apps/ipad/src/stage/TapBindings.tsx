import { MODIFIER_KEYS, VK, type KeyName } from '@vision-pro/protocol';
import type { KeyboardEvent } from 'react';
import { isShortcutModifier, normalizeShortcut, shortcutFromKeyboard, shortcutKeyLabel, shortcutLabel } from '../lib/shortcuts.ts';
import type { Preferences } from '../lib/storage.ts';
import { updatePrefs } from '../lib/store.ts';

const primaryKeys = (Object.keys(VK) as KeyName[]).filter(key => !isShortcutModifier(key));

export function TapBindings({ prefs }: { prefs: Preferences }) {
  return <>
    <label className="creative-tap-setting"><span>Clic</span>
      <select value={prefs.creativeTap} onChange={event => updatePrefs({ creativeTap: event.currentTarget.value as Preferences['creativeTap'] })}>
        <option value="palette">Par défaut · Ouvrir la palette</option>
        <option value="click">Clic Windows</option>
        <option value="swap-tool">Pinceau / gomme</option>
        <option value="shortcut">Clic personnalisé</option>
        <option value="none">Aucune action</option>
      </select>
    </label>
    {prefs.creativeTap === 'shortcut' && <ShortcutEditor title="Clic personnalisé" value={prefs.creativeTapKeys} onChange={creativeTapKeys => updatePrefs({ creativeTapKeys })} />}
    <label className="creative-tap-setting"><span>Double tap</span>
      <select value={prefs.creativeDoubleTap} onChange={event => updatePrefs({ creativeDoubleTap: event.currentTarget.value as Preferences['creativeDoubleTap'] })}>
        <option value="swap-tool">Par défaut · Pinceau / gomme</option>
        <option value="palette">Ouvrir la palette</option>
        <option value="shortcut">Double tap personnalisé</option>
        <option value="none">Aucune action</option>
      </select>
    </label>
    {prefs.creativeDoubleTap === 'shortcut' && <ShortcutEditor title="Double tap personnalisé" value={prefs.creativeDoubleTapKeys} onChange={creativeDoubleTapKeys => updatePrefs({ creativeDoubleTapKeys })} />}
  </>;
}

function ShortcutEditor({ title, value, onChange }: {
  title: string; value: readonly KeyName[]; onChange: (keys: KeyName[]) => void;
}) {
  const primary = value.find(key => !isShortcutModifier(key)) ?? 'Z';
  const modifiers = MODIFIER_KEYS.filter(key => value.includes(key));

  function capture(event: KeyboardEvent<HTMLInputElement>) {
    // Tab alone keeps its usual focus navigation. It can also be picked below.
    if (event.key === 'Tab' && !event.ctrlKey && !event.altKey && !event.metaKey) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat || event.nativeEvent.isComposing) return;
    const keys = shortcutFromKeyboard(event);
    if (keys) onChange(keys);
  }

  return <fieldset className="tap-shortcut">
    <legend>{title}</legend>
    <label className="tap-shortcut-capture">
      <span>Saisir au clavier</span>
      <input className="vp-input" type="text" inputMode="none" readOnly value={shortcutLabel(value)} onKeyDown={capture} />
    </label>
    <div className="tap-shortcut-modifiers" role="group" aria-label="Touches de modification">
      {MODIFIER_KEYS.map(key => <button className="vp-button" type="button" key={key} aria-pressed={modifiers.includes(key)}
        onClick={() => onChange(normalizeShortcut(modifiers.includes(key) ? value.filter(item => item !== key) : [...value, key]))}>
        {shortcutKeyLabel(key)}
      </button>)}
    </div>
    <label className="tap-shortcut-key"><span>Touche principale</span>
      <select value={primary} onChange={event => onChange(normalizeShortcut([...modifiers, event.currentTarget.value]))}>
        {primaryKeys.map(key => <option key={key} value={key}>{shortcutKeyLabel(key)}</option>)}
      </select>
    </label>
    <small>Enregistré sur cet iPad.</small>
  </fieldset>;
}
