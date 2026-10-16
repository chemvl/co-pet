// Pet animation state machine (plan Step 5.1; guidelines "Interrupt rules";
// docs/design/canvas/Inputs.dc.html). Pure logic: no React, no DOM. Time and
// timers are injected so tests can drive them.
import type { CategoryId, PetInput } from '../shared/types';

export type Trigger = PetInput;
export type Flag = 'dragOver' | 'thinking' | 'sleep';
export type Body = Trigger | 'idle' | Flag;
export type Fx =
  | null
  | { kind: 'category'; category: CategoryId }
  | { kind: 'question' }
  | { kind: 'cloud' }
  | { kind: 'zzz' };

export interface PetOutput {
  body: Body;
  fx: Fx;
  /** fidget variant (0 blink, 1 ear flick, 2 tail, 3 wink); 0 otherwise */
  variant: number;
  /** increments every time a reaction starts, so the renderer can restart its animation */
  seq: number;
}

export interface TriggerOptions {
  category?: CategoryId;
  variant?: number;
}

type Priority = 0 | 1 | 2 | 3;

export const TRIGGERS: Readonly<Record<Trigger, { priority: Priority; ms: number }>> = {
  poof: { priority: 0, ms: 500 },
  wake: { priority: 1, ms: 400 },
  catch: { priority: 1, ms: 600 },
  handOver: { priority: 1, ms: 300 },
  camera: { priority: 1, ms: 400 },
  confused: { priority: 2, ms: 600 },
  idea: { priority: 2, ms: 500 },
  tilt: { priority: 2, ms: 500 },
  gesture: { priority: 3, ms: 300 },
  fidget: { priority: 3, ms: 1200 },
};

/** Length of a reaction; the wink (fidget variant 3) is shorter than the other fidgets. */
export function triggerMs(trigger: Trigger, variant = 0): number {
  return trigger === 'fidget' && variant === 3 ? 800 : TRIGGERS[trigger].ms;
}

export const QUEUE_TTL_MS = 2000;
export const LOOP_DELAY_MS = 400;
export const WAKE_HOLD_MS = 60_000;
export const FIDGET_MIN_MS = 30_000;
export const FIDGET_MAX_MS = 90_000;

export interface MachineDeps {
  now(): number;
  /** Runs `fn` after `ms`; returns a cancel function. */
  schedule(fn: () => void, ms: number): () => void;
  /** Called whenever the output changes. */
  onChange(out: PetOutput): void;
  /** Called at 50% of `poof`: apply the pending style, skin or size. */
  onSwap?(): void;
  random?(): number;
}

export interface PetMachine {
  fire(trigger: Trigger, opts?: TriggerOptions): void;
  set(flag: Flag, value: boolean): void;
  setHidden(hidden: boolean): void;
  setPaused(paused: boolean): void;
  output(): PetOutput;
  dispose(): void;
}

interface Reaction {
  trigger: Trigger;
  priority: Priority;
  opts: TriggerOptions;
}

interface Queued extends Reaction {
  at: number;
}

const noop = () => {};

export function createPetMachine(deps: MachineDeps): PetMachine {
  const random = deps.random ?? Math.random;
  const flags = { dragOver: false, thinking: false, sleep: false };
  let thinkingSince = 0;
  let awakeUntil = -Infinity;
  let hidden = false;
  let paused = false;
  let playing: Reaction | null = null;
  let queued: Queued | null = null;
  let seq = 0;
  let out: PetOutput = { body: 'idle', fx: null, variant: 0, seq: 0 };

  let cancelEnd = noop;
  let cancelSwap = noop;
  let cancelThinking = noop;
  let cancelAwake = noop;
  let cancelFidget = noop;
  let fidgetArmed = false;

  function resolveLoop(): Body {
    if (flags.dragOver) return 'dragOver';
    // `thinking` counts only after it has been on for 400 ms, so quick jobs don't flicker.
    if (flags.thinking && deps.now() - thinkingSince >= LOOP_DELAY_MS) return 'thinking';
    if (flags.sleep && deps.now() >= awakeUntil) return 'sleep';
    return 'idle';
  }

  function emit(next: PetOutput) {
    if (
      next.body === out.body &&
      next.variant === out.variant &&
      next.seq === out.seq &&
      JSON.stringify(next.fx) === JSON.stringify(out.fx)
    ) {
      return;
    }
    out = next;
    deps.onChange(out);
  }

  function render() {
    if (paused) {
      emit({ body: 'idle', fx: null, variant: 0, seq });
    } else if (playing) {
      const { trigger, opts } = playing;
      let fx: Fx = null;
      if (trigger === 'idea') fx = { kind: 'category', category: opts.category ?? 'idea' };
      else if (trigger === 'tilt') fx = { kind: 'question' };
      emit({ body: trigger, fx, variant: trigger === 'fidget' ? (opts.variant ?? 0) : 0, seq });
    } else {
      const body = resolveLoop();
      const fx: Fx = body === 'thinking' ? { kind: 'cloud' } : body === 'sleep' ? { kind: 'zzz' } : null;
      emit({ body, fx, variant: 0, seq });
    }
    updateFidgetTimer();
  }

  function updateFidgetTimer() {
    const want = !paused && !hidden && !playing && out.body === 'idle';
    if (want === fidgetArmed) return;
    fidgetArmed = want;
    if (!want) {
      cancelFidget();
      cancelFidget = noop;
      return;
    }
    const delay = FIDGET_MIN_MS + random() * (FIDGET_MAX_MS - FIDGET_MIN_MS);
    cancelFidget = deps.schedule(() => {
      cancelFidget = noop;
      fidgetArmed = false;
      fire('fidget', { variant: Math.min(2, Math.floor(random() * 3)) });
      // If the fidget could not play, re-arm for the next interval.
      updateFidgetTimer();
    }, delay);
  }

  function stopPlaying() {
    cancelEnd();
    cancelSwap();
    cancelEnd = noop;
    cancelSwap = noop;
    playing = null;
  }

  function play(r: Reaction) {
    stopPlaying();
    playing = r;
    seq += 1;
    if (r.trigger === 'wake') {
      awakeUntil = deps.now() + WAKE_HOLD_MS;
      cancelAwake();
      // Re-resolve when the wake hold runs out so a still-set `sleep` takes over.
      cancelAwake = deps.schedule(() => {
        cancelAwake = noop;
        if (!playing) render();
      }, WAKE_HOLD_MS);
    }
    const ms = triggerMs(r.trigger, r.opts.variant);
    if (r.trigger === 'poof') {
      cancelSwap = deps.schedule(() => {
        cancelSwap = noop;
        deps.onSwap?.();
      }, ms / 2);
    }
    cancelEnd = deps.schedule(() => {
      cancelEnd = noop;
      finish();
    }, ms);
    render();
  }

  function finish() {
    playing = null;
    cancelSwap();
    cancelSwap = noop;
    const next = queued;
    queued = null;
    if (next && deps.now() - next.at <= QUEUE_TTL_MS) {
      play(next);
    } else {
      render();
    }
  }

  function enqueue(r: Reaction) {
    // One slot: the newest of the highest priority.
    if (queued && queued.priority < r.priority) return;
    queued = { ...r, at: deps.now() };
  }

  function fire(trigger: Trigger, opts: TriggerOptions = {}) {
    if (paused) {
      // Frozen: no motion, but a pending style/skin/size change still applies.
      if (trigger === 'poof') deps.onSwap?.();
      return;
    }
    if (!playing && out.body === 'sleep' && trigger !== 'wake' && trigger !== 'poof') return;
    const priority = TRIGGERS[trigger].priority;
    const r: Reaction = { trigger, priority, opts };
    if (!playing) {
      play(r);
      return;
    }
    const current = playing.priority;
    if (priority === 3) return;
    if (priority === 0 || current >= priority) {
      play(r);
      return;
    }
    enqueue(r);
  }

  function set(flag: Flag, value: boolean) {
    if (flags[flag] === value) return;
    flags[flag] = value;
    if (flag === 'thinking') {
      cancelThinking();
      cancelThinking = noop;
      if (value) {
        thinkingSince = deps.now();
        cancelThinking = deps.schedule(() => {
          cancelThinking = noop;
          if (!playing) render();
        }, LOOP_DELAY_MS);
      }
    }
    // Loop changes never cut a reaction; Resolve picks them up when it ends.
    if (!playing) render();
  }

  return {
    fire,
    set,
    setHidden(value) {
      if (hidden === value) return;
      hidden = value;
      updateFidgetTimer();
    },
    setPaused(value) {
      if (paused === value) return;
      paused = value;
      if (paused) {
        stopPlaying();
        queued = null;
      }
      render();
    },
    output: () => out,
    dispose() {
      stopPlaying();
      queued = null;
      cancelThinking();
      cancelAwake();
      cancelFidget();
      fidgetArmed = false;
    },
  };
}
