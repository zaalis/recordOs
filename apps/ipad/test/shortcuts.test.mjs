import assert from 'node:assert/strict';
import test from 'node:test';
import { shortcutFromKeyboard, shortcutLabel } from '../src/lib/shortcuts.ts';

test('keyboard capture uses the typed letter on AZERTY and shows French labels', () => {
  const keys = shortcutFromKeyboard({
    key: 'z', code: 'KeyW', ctrlKey: true, shiftKey: true, altKey: false, metaKey: false,
  });
  assert.deepEqual(keys, ['Ctrl', 'Shift', 'Z']);
  assert.equal(shortcutLabel(keys), 'Ctrl + Maj + Z');
});

test('holding a modifier alone does not replace the saved shortcut', () => {
  assert.equal(shortcutFromKeyboard({
    key: 'Control', code: 'ControlLeft', ctrlKey: true, shiftKey: false, altKey: false, metaKey: false,
  }), undefined);
});
