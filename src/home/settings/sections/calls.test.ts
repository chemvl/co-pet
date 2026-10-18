import { describe, expect, it } from 'vitest';
import { translate, type TFunction } from '../../../shared/i18n';
import type { CallDetectionStatus } from '../../../shared/types';
import { callsStatusLine } from './calls';

const en: TFunction = (key, params) => translate('en', key, params);
const vi: TFunction = (key, params) => translate('vi', key, params);

const status = (patch: Partial<CallDetectionStatus>): CallDetectionStatus => ({
  mode: 'ask',
  perProcessMic: true,
  titles: 'screenRecording',
  active: null,
  ...patch,
});
const active = { callId: 'c1', app: 'meet' as const, name: 'Weekly sync', mode: 'ask' as const };

describe('callsStatusLine', () => {
  it('says nothing when all is well', () => {
    expect(callsStatusLine(status({}), en)).toBeNull();
  });

  it('puts an active call first', () => {
    const s = status({ active, titles: 'none', perProcessMic: false });
    expect(callsStatusLine(s, en)).toEqual({ text: 'In a Google Meet call now.', needsPermission: false });
    expect(callsStatusLine(s, vi)?.text).toBe('Đang trong cuộc gọi Google Meet.');
  });

  it('asks for permission before the older-Mac note', () => {
    const s = status({ titles: 'none', perProcessMic: false });
    expect(callsStatusLine(s, en)).toEqual({ text: en('settings.calls.meetNeedsPermission'), needsPermission: true });
    expect(callsStatusLine(s, vi)?.text).toMatch(/^Cần quyền Ghi màn hình/);
  });

  it('notes an older Mac last', () => {
    const s = status({ perProcessMic: false });
    expect(callsStatusLine(s, en)).toEqual({ text: en('settings.calls.olderMac'), needsPermission: false });
    expect(callsStatusLine(s, vi)?.text).toMatch(/^Trên phiên bản macOS này/);
  });
});
