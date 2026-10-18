import { describe, expect, it } from 'vitest';
import { translate, type Lang, type TFunction } from '../shared/i18n';
import type { CallApp, CallDetected, CallEnded, CaptureResult } from '../shared/types';
import {
  callAutoNotice, callEndedNotice, callOfferNotice, diaryNotice, NOTICE_MS, noticeFor, recordingEndedNotice,
} from './notices';

const tIn =
  (lang: Lang): TFunction =>
  (key, params) =>
    translate(lang, key, params);
const t = tIn('en');

const saved = (patch: Partial<CaptureResult> = {}): CaptureResult => ({
  status: 'saved',
  itemIds: ['i1'],
  message: null,
  messageParams: {},
  transcribeItemId: null,
  ...patch,
});

describe('noticeFor', () => {
  it('offers to transcribe a dropped audio file', () => {
    const r = saved({
      message: 'notice.offerTranscribe',
      messageParams: { title: 'standup.m4a' },
      transcribeItemId: 'i1',
    });
    expect(noticeFor(r, t)).toEqual({
      message: 'Transcribe “standup.m4a”?',
      transcribeItemId: 'i1',
      ms: 8000,
    });
  });

  it('says nothing about a plain saved result', () => {
    expect(noticeFor(saved(), t)).toBeNull();
  });
});

describe('diaryNotice', () => {
  it('opens the Pet room with an Open diary action', () => {
    expect(diaryNotice(t)).toEqual({
      message: "Last week's diary is ready.",
      home: 'pet',
      homeLabel: 'Open diary',
      ms: 8000,
    });
  });
});

const detected = (app: CallApp, mode: CallDetected['mode'] = 'ask'): CallDetected => ({
  callId: `${app}:1`,
  app,
  name: app === 'meet' ? 'Standup' : null,
  mode,
});
const ended = (app: CallApp, stopAtMs: number | null): CallEnded => ({ callId: `${app}:1`, app, stopAtMs });

// Every call notice in both languages, for both apps.
const COPY: Record<Lang, Record<CallApp, { offer: string; auto: string; stopping: string; ask: string }>> = {
  en: {
    zoom: {
      offer: 'Zoom call started. Record it?',
      auto: "Recording the Zoom call. Let everyone know they're being recorded.",
      stopping: 'The Zoom call ended. Recording stops in 20 s.',
      ask: 'The Zoom call ended. Stop recording?',
    },
    meet: {
      offer: 'Google Meet call started. Record it?',
      auto: "Recording the Google Meet call. Let everyone know they're being recorded.",
      stopping: 'The Google Meet call ended. Recording stops in 20 s.',
      ask: 'The Google Meet call ended. Stop recording?',
    },
  },
  vi: {
    zoom: {
      offer: 'Cuộc gọi Zoom đã bắt đầu. Ghi âm cuộc gọi này?',
      auto: 'Đang ghi âm cuộc gọi Zoom. Hãy báo cho mọi người biết họ đang được ghi âm.',
      stopping: 'Cuộc gọi Zoom đã kết thúc. Bản ghi sẽ dừng sau 20 giây.',
      ask: 'Cuộc gọi Zoom đã kết thúc. Dừng ghi âm?',
    },
    meet: {
      offer: 'Cuộc gọi Google Meet đã bắt đầu. Ghi âm cuộc gọi này?',
      auto: 'Đang ghi âm cuộc gọi Google Meet. Hãy báo cho mọi người biết họ đang được ghi âm.',
      stopping: 'Cuộc gọi Google Meet đã kết thúc. Bản ghi sẽ dừng sau 20 giây.',
      ask: 'Cuộc gọi Google Meet đã kết thúc. Dừng ghi âm?',
    },
  },
};

describe.each(['en', 'vi'] as const)('call notices (%s)', (lang) => {
  const tl = tIn(lang);

  describe.each(['zoom', 'meet'] as const)('%s', (app) => {
    const copy = COPY[lang][app];

    it('offers to record a detected call for 30 s', () => {
      expect(callOfferNotice(detected(app), tl)).toEqual({
        message: copy.offer,
        record: { callId: `${app}:1` },
        ms: 30_000,
      });
    });

    it('reminds to tell everyone when it records automatically', () => {
      expect(callAutoNotice(detected(app, 'auto'), tl)).toEqual({
        message: copy.auto,
        stopRecording: true,
        ms: 8_000,
      });
    });

    it('counts down to the auto-stop with a Keep recording action', () => {
      const now = 1_000_000;
      expect(callEndedNotice(ended(app, now + 20_000), tl, now)).toEqual({
        message: copy.stopping,
        keepRecording: { callId: `${app}:1` },
        ms: 20_000,
      });
    });

    it('asks to stop when nothing stops the recording on its own', () => {
      expect(callEndedNotice(ended(app, null), tl, 1_000_000)).toEqual({
        message: copy.ask,
        stopRecording: true,
        ms: 30_000,
      });
    });
  });

  it('says why the recording stopped when its call ended', () => {
    expect(recordingEndedNotice('callEnded', tl)).toEqual({
      message:
        lang === 'en'
          ? 'The call ended, so recording stopped. Co-pet is transcribing it.'
          : 'Cuộc gọi đã kết thúc nên bản ghi đã dừng. Co-pet đang chuyển thành văn bản.',
    });
  });
});

describe('callEndedNotice', () => {
  it('stays up at least a plain notice when the stop is (nearly) due', () => {
    const now = 1_000_000;
    const due = callEndedNotice(ended('zoom', now + 300), t, now);
    expect(due.ms).toBe(NOTICE_MS);
    expect(due.message).toBe('The Zoom call ended. Recording stops in 1 s.');
    const overdue = callEndedNotice(ended('zoom', now - 2_000), t, now);
    expect(overdue.ms).toBe(NOTICE_MS);
    expect(overdue.message).toBe('The Zoom call ended. Recording stops in 1 s.');
  });
});

describe('recordingEndedNotice', () => {
  it('keeps the existing maxLength and tooShort notices', () => {
    expect(recordingEndedNotice('maxLength', t)).toEqual({
      message: 'Recording stopped after 4 hours. Co-pet is transcribing it.',
    });
    expect(recordingEndedNotice('tooShort', t)).toEqual({ message: 'That recording was too short to keep.' });
  });
});
