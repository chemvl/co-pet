import { describe, expect, test } from 'vitest';
import { acceleratorFromEvent, acceleratorKeys, modKey } from './accelerator';

const ev = (code: string, m: Partial<Record<'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey', boolean>>) =>
  ({ code, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...m });

describe('acceleratorFromEvent', () => {
  test('macOS: ⌘ is CommandOrControl and ⌃ is Control', () => {
    expect(acceleratorFromEvent(ev('KeyC', { metaKey: true, shiftKey: true }), 'macos')).toEqual({ kind: 'ok', accelerator: 'CommandOrControl+Shift+C' });
    expect(acceleratorFromEvent(ev('KeyC', { ctrlKey: true }), 'macos')).toEqual({ kind: 'ok', accelerator: 'Control+C' });
  });
  test('Windows: Ctrl is CommandOrControl so defaults round-trip; the Win key is refused', () => {
    expect(acceleratorFromEvent(ev('Space', { ctrlKey: true, shiftKey: true }), 'windows')).toEqual({ kind: 'ok', accelerator: 'CommandOrControl+Shift+Space' });
    expect(acceleratorFromEvent(ev('KeyC', { metaKey: true }), 'windows')).toEqual({ kind: 'invalid' });
    expect(acceleratorFromEvent(ev('KeyR', { ctrlKey: true, altKey: true }), 'windows')).toEqual({ kind: 'ok', accelerator: 'CommandOrControl+Alt+R' });
  });
});

describe('acceleratorKeys', () => {
  test('Windows shows words, macOS shows glyphs', () => {
    expect(acceleratorKeys('CommandOrControl+Shift+Space', 'Space', 'windows')).toEqual(['Ctrl', 'Shift', 'Space']);
    expect(acceleratorKeys('CommandOrControl+Alt+Enter', 'Space', 'windows')).toEqual(['Ctrl', 'Alt', 'Enter']);
    expect(acceleratorKeys('CommandOrControl+Shift+Space', 'Space', 'macos')).toEqual(['⌘', '⇧', 'Space']);
  });
  test('modKey', () => {
    expect([modKey('macos'), modKey('windows')]).toEqual(['⌘', 'Ctrl']);
  });
});
