// Plan Step 5.2 pose table: machine body → cat parts + CSS motion (src/pet/cat.css).
// One-shot durations equal the trigger lengths in petMachine.
import type { CatFx } from './CatArt';
import { IDLE_BLINK_VARIANT, triggerMs, type Body, type Fx } from './petMachine';

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

/** `shut`: the eyes drawn closed for the whole length, with no CSS animation (the still idle's blink).
 *  A motion rather than closed `parts` so CatArt can drop it under reduced motion, like `blink`. */
export type CatMotion =
  | 'breathe' | 'stretch' | 'hop' | 'lean' | 'bounce' | 'offer' | 'flash' | 'shake' | 'tilt'
  | 'blink' | 'shut' | 'ear' | 'tail' | 'dots' | 'poof';

export interface CatPose {
  parts: CatParts;
  motion: CatMotion | null;
  /** one-shot length, or one loop period */
  ms: number;
  /** repeat a one-shot motion (`carrying` bounces until the export ends) */
  loop?: boolean;
}

export interface PoseOptions {
  /** false: idle and the held sign hold still instead of the `breathe` loop (the Windows desktop pet;
   *  see `MachineDeps.stillIdle`). Default true. */
  breathe?: boolean;
}

const BREATHE_MS = 3000;
const LEAN_MS = 800;
const DOTS_MS = 1200;
const CARRY_MS = 600;

const PLAIN: CatParts = { eyes: 'open', mouth: 'smile', item: false, dots: false, zzz: false };

export function poseFor(body: Body, variant = 0, opts: PoseOptions = {}): CatPose {
  const parts = (p: Partial<CatParts>): CatParts => ({ ...PLAIN, ...p });
  const breathe = opts.breathe === false ? { motion: null, ms: 0 } : { motion: 'breathe' as const, ms: BREATHE_MS };
  switch (body) {
    case 'idle':
      return { parts: PLAIN, ...breathe };
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
    case 'glance':
      return { parts: parts({ eyes: 'up' }), motion: 'ear', ms: triggerMs('glance') };
    case 'holdSign':
      return { parts: parts({ eyes: 'happy', item: true }), ...breathe };
    case 'fidget':
      if (variant === 3) return { parts: parts({ eyes: 'wink' }), motion: null, ms: triggerMs('fidget', 3) };
      if (variant === IDLE_BLINK_VARIANT) return { parts: PLAIN, motion: 'shut', ms: triggerMs('fidget', variant) };
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

/** The machine's fx the art draws above the head (category, question, agent); the rest is drawn by poses. */
export function fxForArt(fx: Fx): CatFx {
  return fx && (fx.kind === 'category' || fx.kind === 'question' || fx.kind === 'agent') ? fx : null;
}
