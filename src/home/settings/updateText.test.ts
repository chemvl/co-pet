import { describe, expect, it } from 'vitest';
import { translate, type TFunction } from '../../shared/i18n';
import type { UpdateState, UpdateStatus } from '../../shared/types';
import { updateLine } from './updateText';

const t: TFunction = (key, params) => translate('en', key, params);
const at = (state: UpdateState, extra: Partial<UpdateStatus> = {}): UpdateStatus => ({
  state, version: null, notes: null, progress: null, error: null, ...extra,
});

describe('updateLine', () => {
  it('gives each state its line', () => {
    expect(updateLine(at('idle'), t)).toBeNull();
    expect(updateLine(at('checking'), t)).toBe('Checking for updates…');
    expect(updateLine(at('downloading', { progress: 0.424 }), t)).toBe('Downloading 42%');
    expect(updateLine(at('ready', { version: '0.9.1' }), t)).toBe('Version 0.9.1 is ready');
    expect(updateLine(at('upToDate'), t)).toBe('Up to date');
    expect(updateLine(at('failed', { error: 'updateFailed' }), t)).toBe("Couldn't update. Try again later.");
  });
});
