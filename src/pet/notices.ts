// CaptureResult → what the pet says about it (plan Steps 5.7 and 6).
import type { I18nKey, TFunction } from '../shared/i18n';
import { en } from '../shared/i18n/en';
import type { CaptureResult } from '../shared/types';

export type PermissionKind = 'screenRecording' | 'accessibility' | 'microphone';

export interface Notice {
  message: string;
  /** the one quiet action: opens the privacy pane for a missing permission… */
  permission?: PermissionKind;
  /** …or transcribes this dropped audio/video item and opens its meeting… */
  transcribeItemId?: string;
  /** …or opens Home at this route */
  home?: string;
  /** label of the `home` action; `common.openHome` when unset */
  homeLabel?: string;
  /** how long it stays up; `NOTICE_MS` when unset */
  ms?: number;
}

const PERMISSION_OF: Partial<Record<string, PermissionKind>> = {
  'notice.needsScreenRecording': 'screenRecording',
  'notice.needsAccessibility': 'accessibility',
};

/** Offers stay longer than a plain notice: they ask for a decision. */
const OFFER_MS = 8000;

const isNoticeKey = (s: string): s is I18nKey => s.startsWith('notice.') && s in en;

/** The PetNotice for a capture result, or null when there is nothing to tell. */
export function noticeFor(r: CaptureResult, t: TFunction): Notice | null {
  if (r.status === 'saved' && r.transcribeItemId) {
    return {
      message: t('notice.offerTranscribe', { title: r.messageParams.title ?? '' }),
      transcribeItemId: r.transcribeItemId,
      ms: OFFER_MS,
    };
  }
  if (r.status === 'failed') {
    const reason = r.message && isNoticeKey(r.message) ? t(r.message, r.messageParams) : (r.message ?? '');
    return { message: t('notice.captureFailed', { reason }) };
  }
  if (r.status !== 'skipped' || !r.message) return null;
  if (!isNoticeKey(r.message)) return { message: t('notice.captureFailed', { reason: r.message }) };
  return { message: t(r.message, r.messageParams), permission: PERMISSION_OF[r.message] };
}

/** Last week's diary is ready: opens the Pet room (`#/pet`), where the diary lives. */
export function diaryNotice(t: TFunction): Notice {
  return { message: t('notice.diaryReady'), home: 'pet', homeLabel: t('notice.openDiary'), ms: OFFER_MS };
}
