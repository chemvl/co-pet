// Plan Step 5.2 pose table: machine body → cat parts + CSS motion (src/pet/cat.css).
// One-shot durations equal the trigger lengths in petMachine.
import { triggerMs, type Body } from './petMachine';

export type CatEyes = 'open' | 'wide' | 'up' | 'closed' | 'happy' | 'wink';

export interface CatParts {
  eyes: CatEyes;
  mouth: 'smile' | 'open';
  /** the caught item held against the belly */
  item: boolean;
  /** thought-cloud dots */
  dots: boolean;
  zzz: boolean;
}

export type CatMotion =
  | 'breathe' | 'stretch' | 'hop' | 'lean' | 'bounce' | 'offer' | 'flash' | 'shake' | 'tilt'
  | 'blink' | 'ear' | 'tail' | 'dots' | 'poof';

export interface CatPose {
  parts: CatParts;
  motion: CatMotion | null;
  /** one-shot length, or one loop period */
  ms: number;
  /** repeat a one-shot motion (`carrying` bounces until the export ends) */
  loop?: boolean;
}

const BREATHE_MS = 3000;
const LEAN_MS = 800;
const DOTS_MS = 1200;
const CARRY_MS = 600;

const PLAIN: CatParts = { eyes: 'open', mouth: 'smile', item: false, dots: false, zzz: false };

export function poseFor(body: Body, variant = 0): CatPose {
  const parts = (p: Partial<CatParts>): CatParts => ({ ...PLAIN, ...p });
  switch (body) {
    case 'idle':
      return { parts: PLAIN, motion: 'breathe', ms: BREATHE_MS };
    case 'wake':
      return { parts: parts({ eyes: 'wide' }), motion: 'stretch', ms: triggerMs('wake') };
    case 'gesture':
      return { parts: parts({ eyes: 'happy' }), motion: 'hop', ms: triggerMs('gesture') };
    case 'dragOver':
      return { parts: parts({ eyes: 'wide', mouth: 'open' }), motion: 'lean', ms: LEAN_MS };
    case 'catch':
      return { parts: parts({ eyes: 'wide', mouth: 'open', item: true }), motion: 'bounce', ms: triggerMs('catch') };
    case 'handOver':
      return { parts: parts({ eyes: 'happy', item: true }), motion: 'offer', ms: triggerMs('handOver') };
    case 'camera':
      return { parts: PLAIN, motion: 'flash', ms: triggerMs('camera') };
    case 'confused':
      return { parts: parts({ eyes: 'up' }), motion: 'shake', ms: triggerMs('confused') };
    case 'idea':
      return { parts: parts({ eyes: 'happy' }), motion: 'hop', ms: triggerMs('idea') };
    case 'tilt':
      return { parts: parts({ eyes: 'up' }), motion: 'tilt', ms: triggerMs('tilt') };
    case 'celebrate':
      return { parts: parts({ eyes: 'happy', mouth: 'open' }), motion: 'hop', ms: triggerMs('celebrate') };
    case 'cheer':
      return { parts: parts({ eyes: 'happy' }), motion: 'tail', ms: triggerMs('cheer') };
    case 'holdSign':
      return { parts: parts({ eyes: 'happy', item: true }), motion: 'breathe', ms: BREATHE_MS };
    case 'fidget':
      if (variant === 3) return { parts: parts({ eyes: 'wink' }), motion: null, ms: triggerMs('fidget', 3) };
      return {
        parts: PLAIN,
        motion: variant === 1 ? 'ear' : variant === 2 ? 'tail' : 'blink',
        ms: triggerMs('fidget', variant),
      };
    case 'carrying':
      return { parts: parts({ item: true }), motion: 'bounce', ms: CARRY_MS, loop: true };
    case 'thinking':
      return { parts: parts({ eyes: 'up', dots: true }), motion: 'dots', ms: DOTS_MS };
    case 'sleep':
      return { parts: parts({ eyes: 'closed', zzz: true }), motion: null, ms: 0 };
    case 'poof':
      return { parts: PLAIN, motion: 'poof', ms: triggerMs('poof') };
  }
}
