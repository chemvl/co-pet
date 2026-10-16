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
