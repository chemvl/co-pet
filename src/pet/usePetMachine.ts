// React binding for the pet machine: real time and timers, output as state.
import { useEffect, useRef, useState } from 'react';
import { createPetMachine, type PetMachine, type PetOutput } from './petMachine';

export function usePetMachine(onSwap: () => void): { machine: PetMachine; output: PetOutput } {
  const swapRef = useRef(onSwap);
  swapRef.current = onSwap;
  const [output, setOutput] = useState<PetOutput>({ body: 'idle', fx: null, variant: 0, seq: 0 });
  const [machine] = useState(() =>
    createPetMachine({
      now: () => performance.now(),
      schedule: (fn, ms) => {
        const id = window.setTimeout(fn, ms);
        return () => window.clearTimeout(id);
      },
      onChange: setOutput,
      onSwap: () => swapRef.current(),
    }),
  );
  useEffect(() => () => machine.dispose(), [machine]);
  return { machine, output };
}

/** The machine and the look it shows. When `key` changes after the first look, the pet poofs and
 *  `wanted` swaps in at the poof's midpoint (at once while paused); `onSwapped` runs after the swap. */
export function usePetLook<T>(
  wanted: T | null,
  key: string | null,
  onSwapped?: () => void,
): { machine: PetMachine; output: PetOutput; applied: T | null } {
  const [applied, setApplied] = useState<T | null>(null);
  const pending = useRef<T | null>(null);
  const shownKey = useRef<string | null>(null);
  const onSwappedRef = useRef(onSwapped);
  onSwappedRef.current = onSwapped;
  const { machine, output } = usePetMachine(() => {
    if (pending.current === null) return;
    setApplied(pending.current);
    pending.current = null;
    onSwappedRef.current?.();
  });
  useEffect(() => {
    if (wanted === null || key === null || key === shownKey.current) return;
    if (shownKey.current === null) {
      shownKey.current = key;
      setApplied(wanted);
      return;
    }
    shownKey.current = key;
    pending.current = wanted;
    machine.fire('poof');
  }, [key]); // `wanted` is rebuilt every render; its key says when it changed
  return { machine, output, applied };
}
