// Global-shortcut accelerators (tauri-plugin-global-shortcut syntax) ↔ keyboard events and Kbd keys.

const MODIFIER_CODES: Record<string, true> = {
  MetaLeft: true, MetaRight: true, ControlLeft: true, ControlRight: true, AltLeft: true, AltRight: true,
  ShiftLeft: true, ShiftRight: true, CapsLock: true, Fn: true, FnLock: true, OSLeft: true, OSRight: true,
};

const NAMED_CODES: Record<string, true> = {
  Space: true, Enter: true, Tab: true, Backspace: true, Delete: true, Home: true, End: true, PageUp: true,
  PageDown: true, ArrowUp: true, ArrowDown: true, ArrowLeft: true, ArrowRight: true, Minus: true, Equal: true,
  BracketLeft: true, BracketRight: true, Backslash: true, Semicolon: true, Quote: true, Comma: true, Period: true,
  Slash: true, Backquote: true,
};

export type RecordResult =
  | { kind: 'wait' } // only modifiers so far
  | { kind: 'invalid' } // a key without ⌘, ⌃ or ⌥
  | { kind: 'ok'; accelerator: string };

/** Maps a keydown to an accelerator; `⌘`→`CommandOrControl`, `⌃`→`Control`, `⌥`→`Alt`, `⇧`→`Shift`. */
export function acceleratorFromEvent(e: Pick<KeyboardEvent, 'code' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'>): RecordResult {
  if (Object.hasOwn(MODIFIER_CODES, e.code)) return { kind: 'wait' };
  const key = keyFromCode(e.code);
  if (!key || !(e.metaKey || e.ctrlKey || e.altKey)) return { kind: 'invalid' };
  const mods: string[] = [];
  if (e.metaKey) mods.push('CommandOrControl');
  if (e.ctrlKey) mods.push('Control');
  if (e.altKey) mods.push('Alt');
  if (e.shiftKey) mods.push('Shift');
  return { kind: 'ok', accelerator: [...mods, key].join('+') };
}

function keyFromCode(code: string): string | null {
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1];
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return digit[1];
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  return Object.hasOwn(NAMED_CODES, code) ? code : null;
}

const MOD_GLYPH: Record<string, string> = {
  commandorcontrol: '⌘', cmdorctrl: '⌘', command: '⌘', cmd: '⌘', super: '⌘', meta: '⌘',
  control: '⌃', ctrl: '⌃', alt: '⌥', option: '⌥', shift: '⇧',
};

const KEY_GLYPH: Record<string, string> = {
  enter: '↩', tab: '⇥', backspace: '⌫', delete: '⌦', home: '↖', end: '↘', pageup: '⇞', pagedown: '⇟',
  arrowup: '↑', up: '↑', arrowdown: '↓', down: '↓', arrowleft: '←', left: '←', arrowright: '→', right: '→',
  minus: '-', equal: '=', bracketleft: '[', bracketright: ']', backslash: '\\', semicolon: ';', quote: "'",
  comma: ',', period: '.', slash: '/', backquote: '`',
};

/** Kbd keys for an accelerator, e.g. `CommandOrControl+Shift+Space` → `['⌘', '⇧', 'Space']`. */
export function acceleratorKeys(accelerator: string, spaceLabel: string): string[] {
  return accelerator
    .split('+')
    .filter(Boolean)
    .map((part) => {
      const p = part.toLowerCase();
      if (Object.hasOwn(MOD_GLYPH, p)) return MOD_GLYPH[p];
      if (p === 'space') return spaceLabel;
      if (Object.hasOwn(KEY_GLYPH, p)) return KEY_GLYPH[p];
      const letter = /^key([a-z])$/.exec(p);
      if (letter) return letter[1].toUpperCase();
      const digit = /^digit([0-9])$/.exec(p);
      if (digit) return digit[1];
      return part.length === 1 ? part.toUpperCase() : part;
    });
}
