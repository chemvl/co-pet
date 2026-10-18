// CaptureResult, recording and call events → what the pet says about them
// (plan Steps 5.7 and 6; auto-record plan Task 4.1).
import type { I18nKey, TFunction } from '../shared/i18n';
import { en } from '../shared/i18n/en';
import type { CallDetected, CallEnded, CaptureResult, RecordingStatus } from '../shared/types';

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
  /** …or records this detected call… */
  record?: { callId: string };
  /** …or keeps this ended call's recording running… */
  keepRecording?: { callId: string };
  /** …or stops the running recording */
  stopRecording?: true;
  /** how long it stays up; `NOTICE_MS` when unset */
  ms?: number;
}

/** How long a plain notice stays up. */
export const NOTICE_MS = 4000;

const PERMISSION_OF: Partial<Record<string, PermissionKind>> = {
  'notice.needsScreenRecording': 'screenRecording',
  'notice.needsAccessibility': 'accessibility',
};

/** Offers stay longer than a plain notice: they ask for a decision. */
const OFFER_MS = 8000;
/** A detected call's offer, or its "stop recording?" question, waits longer: the user is in a call. */
const CALL_ASK_MS = 30_000;

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

/** Ask mode: a Zoom or Meet call started; Record starts recording it. */
export function callOfferNotice(e: CallDetected, t: TFunction): Notice {
  return {
    message: t(e.app === 'zoom' ? 'notice.callOffer.zoom' : 'notice.callOffer.meet'),
    record: { callId: e.callId },
    ms: CALL_ASK_MS,
  };
}

/** A detected call's recording started: the consent reminder, with a way out. */
export function callAutoNotice(e: CallDetected, t: TFunction): Notice {
  const app = t(e.app === 'zoom' ? 'call.app.zoom' : 'call.app.meet');
  return { message: t('notice.callAuto', { app }), stopRecording: true, ms: OFFER_MS };
}

/**
 * A detected call ended. With `stopAtMs` its recording stops on its own: the notice counts
 * down until then (at least `NOTICE_MS`) and offers Keep recording; otherwise it asks.
 */
export function callEndedNotice(e: CallEnded, t: TFunction, now: number): Notice {
  const app = t(e.app === 'zoom' ? 'call.app.zoom' : 'call.app.meet');
  if (e.stopAtMs === null) {
    return { message: t('notice.callEnded.ask', { app }), stopRecording: true, ms: CALL_ASK_MS };
  }
  const left = e.stopAtMs - now;
  return {
    message: t('notice.callEnded.stopping', { app, s: Math.max(1, Math.round(left / 1000)) }),
    keepRecording: { callId: e.callId },
    ms: Math.max(left, NOTICE_MS),
  };
}

const ENDED_KEY: Record<NonNullable<RecordingStatus['endedReason']>, I18nKey> = {
  maxLength: 'notice.recMaxLength',
  tooShort: 'notice.recTooShort',
  callEnded: 'notice.recCallEnded',
};

/** Why a recording ended without the user stopping it. */
export function recordingEndedNotice(reason: NonNullable<RecordingStatus['endedReason']>, t: TFunction): Notice {
  return { message: t(ENDED_KEY[reason]) };
}
