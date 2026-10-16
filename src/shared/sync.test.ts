import { describe, expect, it } from 'vitest';
import { translate } from './i18n';
import { canSyncNow, localPillLabel, pendingText, syncPill } from './sync';
import type { SyncState } from './types';

const STATES: SyncState[] = ['off', 'paused', 'needsKey', 'syncing', 'idle', 'offline', 'error', 'signedOut'];

describe('syncPill', () => {
  it('maps every state to a pill with the brief copy', () => {
    const shown = (lang: 'en' | 'vi') =>
      Object.fromEntries(STATES.map((s) => {
        const p = syncPill(s);
        return [s, [p.status, translate(lang, p.label), p.icon ?? null]];
      }));
    expect(shown('en')).toEqual({
      off: ['local', 'Local only', null],
      paused: ['local', 'Local only', null],
      needsKey: ['pending', 'Recovery key needed', null],
      syncing: ['pending', 'Syncing…', 'sync'],
      idle: ['synced', 'Synced', null],
      offline: ['pending', 'Offline · changes saved here', null],
      error: ['failed', 'Sync failed', null],
      signedOut: ['failed', 'Signed out', null],
    });
    expect(Object.values(shown('vi')).map((v) => v[1])).toEqual([
      'Chỉ trên máy', 'Chỉ trên máy', 'Cần khoá khôi phục', 'Đang đồng bộ…', 'Đã đồng bộ',
      'Ngoại tuyến · thay đổi được lưu trên máy', 'Đồng bộ thất bại', 'Đã đăng xuất',
    ]);
  });
});

describe('canSyncNow', () => {
  it('is true only while idle, offline or failed', () => {
    expect(STATES.filter(canSyncNow)).toEqual(['idle', 'offline', 'error']);
    expect(canSyncNow(undefined)).toBe(false);
  });
});

describe('localPillLabel', () => {
  const small = { fileTooLarge: false };
  const big = { fileTooLarge: true };
  it('labels only files over the limit while sync runs', () => {
    for (const s of ['idle', 'syncing', 'offline'] as const) {
      expect(localPillLabel(small, s, true)).toBeNull();
      expect(localPillLabel(big, s, false)).toBe('item.onlyThisMac');
    }
    expect(translate('en', 'item.onlyThisMac')).toBe('Only on this Mac');
    expect(translate('vi', 'item.onlyThisMac')).toBe('Chỉ trên máy này');
  });
  it('keeps the default otherwise', () => {
    for (const s of [undefined, 'off', 'paused', 'needsKey', 'error', 'signedOut'] as const) {
      expect(localPillLabel(big, s, true)).toBe('common.localOnly');
      expect(localPillLabel(small, s, false)).toBeNull();
    }
  });
});

describe('pendingText', () => {
  it('uses the singular for one change', () => {
    const en = (key: Parameters<typeof translate>[1], p?: Record<string, string | number>) => translate('en', key, p);
    expect(pendingText(en, 1)).toBe('1 change waiting');
    expect(pendingText(en, 3)).toBe('3 changes waiting');
    expect(pendingText((k, p) => translate('vi', k, p), 1)).toBe('1 thay đổi đang chờ');
  });
});
