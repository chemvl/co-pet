import { describe, expect, it } from 'vitest';
import { translate, type TFunction } from '../shared/i18n';
import type { CaptureResult } from '../shared/types';
import { noticeFor } from './notices';

const t: TFunction = (key, params) => translate('en', key, params);

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
