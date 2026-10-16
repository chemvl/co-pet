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
    machine.fire('idea', { icon: 'link', color: 'code' });
    expect(machine.output()).toMatchObject({ body: 'idea', fx: { kind: 'category', icon: 'link', color: 'code' } });
    clock.advance(100);
    machine.fire('catch');
    expect(machine.output()).toMatchObject({ body: 'catch', fx: null });
    clock.advance(600);
    expect(body()).toBe('idle');
  });

  it('an idea without a category shows the Idea icon and colour', () => {
    const { machine } = setup();
    machine.fire('idea');
    expect(machine.output().fx).toEqual({ kind: 'category', icon: 'idea', color: 'idea' });
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

  it('celebrate_waits_behind_p1', () => {
    const { clock, machine, body } = setup();
    machine.fire('catch');
    clock.advance(100);
    machine.fire('celebrate');
    expect(body()).toBe('catch');
    clock.advance(500);
    expect(machine.output()).toMatchObject({ body: 'celebrate', fx: null });
    clock.advance(799);
    expect(body()).toBe('celebrate');
    clock.advance(1);
    expect(body()).toBe('idle');
  });

  it('cheer_does_not_cut_catch', () => {
    const { clock, machine, body } = setup();
    machine.fire('catch');
    clock.advance(100);
    machine.fire('cheer');
    expect(body()).toBe('catch');
    clock.advance(500);
    expect(machine.output()).toMatchObject({ body: 'cheer', fx: null });
    clock.advance(599);
    expect(body()).toBe('cheer');
    clock.advance(1);
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
    machine.fire('idea', { icon: 'read', color: 'read' }); // queued at t=0
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

  it('holdSign_loops_until_cleared', () => {
    const { clock, machine, body } = setup();
    machine.set('thinking', true);
    machine.set('holdSign', true);
    expect(machine.output()).toMatchObject({ body: 'holdSign', fx: null });
    clock.advance(10_000);
    expect(body()).toBe('holdSign');
    machine.fire('gesture');
    expect(body()).toBe('gesture');
    clock.advance(300);
    expect(body()).toBe('holdSign');
    machine.set('holdSign', false);
    expect(body()).toBe('thinking');
    machine.set('thinking', false);
    expect(body()).toBe('idle');
  });

  it('carrying_needs_400ms', () => {
    const { clock, machine, body } = setup();
    machine.set('carrying', true);
    expect(body()).toBe('idle');
    clock.advance(399);
    expect(body()).toBe('idle');
    clock.advance(1);
    expect(machine.output()).toMatchObject({ body: 'carrying', fx: null });
    // Above thinking, below the held sign.
    machine.set('thinking', true);
    clock.advance(400);
    expect(body()).toBe('carrying');
    machine.set('holdSign', true);
    expect(body()).toBe('holdSign');
    machine.set('holdSign', false);
    machine.set('carrying', false);
    expect(body()).toBe('thinking');
    // A quick job never shows.
    machine.set('thinking', false);
    machine.set('carrying', true);
    clock.advance(200);
    machine.set('carrying', false);
    clock.advance(400);
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

  it('drops a queued reaction when the pet resolves into sleep', () => {
    const { clock, machine, body } = setup();
    machine.fire('catch');
    machine.fire('idea', { icon: 'code', color: 'code' });
    machine.set('sleep', true);
    clock.advance(600);
    expect(body()).toBe('sleep');
    clock.advance(600);
    expect(body()).toBe('sleep');
  });

  it('expires a stale queued entry before comparing priority', () => {
    const { clock, machine, body } = setup();
    machine.fire('poof');
    machine.fire('catch'); // P1 queued behind P0 at t=0
    // Chain poofs (P0) so the queued catch outlives the 2 s TTL while still playing.
    for (let t = 400; t <= 2000; t += 400) {
      clock.advance(400);
      machine.fire('poof');
    }
    clock.advance(100);
    machine.fire('idea', { icon: 'code', color: 'code' }); // stale catch must not block this P2
    clock.advance(400); // last poof ends at 2500
    expect(body()).toBe('idea');
  });

  it('pausing mid-poof still applies the pending swap once', () => {
    const { clock, machine, swaps } = setup();
    machine.fire('poof');
    clock.advance(100);
    machine.setPaused(true);
    expect(swaps).toEqual([100]);
    clock.advance(1000);
    expect(swaps).toEqual([100]);
  });
});
