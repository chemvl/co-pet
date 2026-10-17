// Pet speaks: which reactions get a line in the pet's own voice, when the pet may speak, and how often.
// Pure: no React, no DOM.
import type { I18nKey } from '../shared/i18n';
import type { PetInput } from '../shared/types';

export type SpeechLine =
  | 'catch' | 'camera' | 'duplicate' | 'idea' | 'tilt' | 'celebrate' | 'cheer' | 'confused' | 'recStopped' | 'newLook';

/** `announce`: read out through the pet's live region (false where PetApp already announces the event). */
export const SPEECH_LINES: Readonly<Record<SpeechLine, { key: I18nKey; announce: boolean; limited: boolean }>> = {
  catch: { key: 'speech.catch', announce: true, limited: true },
  camera: { key: 'speech.camera', announce: true, limited: true },
  duplicate: { key: 'speech.duplicate', announce: false, limited: true },
  idea: { key: 'speech.idea', announce: false, limited: true },
  tilt: { key: 'speech.tilt', announce: false, limited: true },
  celebrate: { key: 'speech.celebrate', announce: true, limited: true },
  cheer: { key: 'speech.cheer', announce: true, limited: true },
  confused: { key: 'speech.confused', announce: true, limited: true },
  recStopped: { key: 'speech.recStopped', announce: true, limited: true },
  newLook: { key: 'speech.newLook', announce: true, limited: false },
};

export const SPEECH_DELAY_MS = 300; // a notice or card that follows the reaction wins
export const SPEECH_MS = 3000;
export const SPEECH_GAP_MS = 20_000;
export const SPEECH_MAX_PER_HOUR = 12;

const HOUR_MS = 3_600_000;
const DIRECT: Partial<Record<PetInput, SpeechLine>> = {
  catch: 'catch', camera: 'camera', idea: 'idea', tilt: 'tilt', celebrate: 'celebrate', cheer: 'cheer', confused: 'confused',
};

/** pet-reaction → line: catch, camera, idea, tilt, celebrate, cheer, confused map 1:1; fidget variant 3 → duplicate;
 *  wake, gesture, handOver, glance, poof, other fidgets → null. */
export function speechFor(r: { input: PetInput; variant?: number }): SpeechLine | null {
  const direct = DIRECT[r.input];
  if (direct) return direct;
  if (r.input === 'fidget' && r.variant === 3) return 'duplicate';
  return null;
}

export interface SpeechContext {
  enabled: boolean;
  paused: boolean;
  hidden: boolean;
  asleep: boolean;
  surfaceOpen: boolean;
  recording: boolean;
  moving: boolean;
  dragOver: boolean;
}

/** enabled && every other field false. */
export function canSpeak(c: SpeechContext): boolean {
  return c.enabled && !c.paused && !c.hidden && !c.asleep && !c.surfaceOpen && !c.recording && !c.moving && !c.dragOver;
}

export interface SpeechLimiter {
  take(line: SpeechLine, now: number): boolean;
}

/** Limited lines: ≥ SPEECH_GAP_MS since the last shown limited line and < SPEECH_MAX_PER_HOUR in the last hour; a
 *  granted take is recorded. Unlimited lines (newLook) always pass and are not recorded. */
export function createSpeechLimiter(): SpeechLimiter {
  let shown: number[] = [];
  return {
    take(line, now) {
      if (!SPEECH_LINES[line].limited) return true;
      shown = shown.filter((at) => now - at < HOUR_MS);
      const last = shown[shown.length - 1];
      if (last !== undefined && now - last < SPEECH_GAP_MS) return false;
      if (shown.length >= SPEECH_MAX_PER_HOUR) return false;
      shown.push(now);
      return true;
    },
  };
}
