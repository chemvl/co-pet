import { describe, expect, it } from 'vitest';
import { createPetMachine, type PetMachine, type PetOutput } from './petMachine';

/** Deterministic clock: timers run in due order as `advance` moves time forward. */
class FakeClock {
  t = 0;
  private timers: { at: number; id: number; fn: () => void }[] = [];
  private nextId = 0;

  now = () => this.t;

  schedule = (fn: () => void, ms: number) => {
    const id = this.nextId++;
    this.timers.push({ at: this.t + ms, id, fn });
    return () => {
      this.timers = this.timers.filter((timer) => timer.id !== id);
    };
  };

  advance(ms: number) {
    const end = this.t + ms;
    for (;;) {
      const due = this.timers
        .filter((timer) => timer.at <= end)
        .sort((a, b) => a.at - b.at || a.id - b.id)[0];
      if (!due) break;
      this.timers = this.timers.filter((timer) => timer !== due);
      this.t = due.at;
      due.fn();
    }
    this.t = end;
  }
}

function setup() {
  const clock = new FakeClock();
  const swaps: number[] = [];
  const machine: PetMachine = createPetMachine({
    now: clock.now,
    schedule: clock.schedule,
    onChange: () => {},
    onSwap: () => swaps.push(clock.t),
    // Keeps the fidget timer at 90 s, outside every test's time window.
    random: () => 0.999,
  });
  const body = (): PetOutput['body'] => machine.output().body;
  return { clock, machine, swaps, body };
}

describe('petMachine interrupt rules', () => {
  it('P1 interrupts a playing P2 at once', () => {
    const { clock, machine, body } = setup();
    machine.fire('idea', { category: 'code' });
    expect(machine.output()).toMatchObject({ body: 'idea', fx: { kind: 'category', category: 'code' } });
    clock.advance(100);
    machine.fire('catch');
    expect(machine.output()).toMatchObject({ body: 'catch', fx: null });
    clock.advance(600);
    expect(body()).toBe('idle');
  });

  it('queues a P2 behind a playing P1 and plays it afterwards', () => {
    const { clock, machine, body } = setup();
    machine.fire('catch');
    clock.advance(100);
    machine.fire('tilt');
    expect(body()).toBe('catch');
    clock.advance(499);
    expect(body()).toBe('catch');
    clock.advance(1);
    expect(machine.output()).toMatchObject({ body: 'tilt', fx: { kind: 'question' } });
    clock.advance(500);
    expect(body()).toBe('idle');
  });

  it('drops a P3 while a reaction is playing', () => {
    const { clock, machine, body } = setup();
    machine.fire('catch');
    clock.advance(100);
    machine.fire('gesture');
    expect(body()).toBe('catch');
    clock.advance(500);
    expect(body()).toBe('idle');
    clock.advance(300);
    expect(body()).toBe('idle');
  });

  it('discards a queued reaction after 2 s', () => {
    const { clock, machine, body } = setup();
    machine.fire('catch');
    machine.fire('idea', { category: 'read' }); // queued at t=0
    // Keep P1s cutting each other until the queued idea is older than 2 s.
    for (const at of [500, 1000, 1500]) {
      clock.advance(at - clock.t);
      machine.fire('catch');
      expect(body()).toBe('catch');
    }
    clock.advance(600); // last catch ends at t=2100
    expect(body()).toBe('idle');
  });
});

describe('petMachine loops', () => {
  it('shows thinking only after 400 ms', () => {
    const { clock, machine, body } = setup();
    machine.set('thinking', true);
    expect(body()).toBe('idle');
    clock.advance(399);
    expect(body()).toBe('idle');
    clock.advance(1);
    expect(machine.output()).toMatchObject({ body: 'thinking', fx: { kind: 'cloud' } });
    machine.set('thinking', false);
    machine.set('thinking', true);
    expect(body()).toBe('idle');
  });

  it('drops catch while asleep', () => {
    const { clock, machine, body } = setup();
    machine.set('sleep', true);
    expect(machine.output()).toMatchObject({ body: 'sleep', fx: { kind: 'zzz' } });
    machine.fire('catch');
    expect(body()).toBe('sleep');
    clock.advance(600);
    expect(body()).toBe('sleep');
  });

  it('wake suppresses sleep for 60 s', () => {
    const { clock, machine, body } = setup();
    machine.set('sleep', true);
    machine.fire('wake');
    expect(body()).toBe('wake');
    clock.advance(400);
    expect(body()).toBe('idle');
    clock.advance(59_599);
    expect(body()).toBe('idle');
    clock.advance(1);
    expect(body()).toBe('sleep');
  });

  it('poof calls onSwap at 50%', () => {
    const { clock, machine, swaps, body } = setup();
    machine.fire('poof');
    clock.advance(249);
    expect(swaps).toEqual([]);
    clock.advance(1);
    expect(swaps).toEqual([250]);
    expect(body()).toBe('poof');
    clock.advance(250);
    expect(body()).toBe('idle');
    expect(swaps).toEqual([250]);
  });
});
