import { describe, expect, it } from 'vitest';
import { poseFor } from './pose';

describe('poseFor', () => {
  it('breathes in idle and holding the sign by default', () => {
    expect(poseFor('idle')).toMatchObject({ motion: 'breathe', ms: 3000 });
    expect(poseFor('holdSign')).toMatchObject({ motion: 'breathe', ms: 3000 });
  });

  it('holds still without breathe (Windows desktop pet)', () => {
    expect(poseFor('idle', 0, { breathe: false })).toMatchObject({ motion: null, parts: { eyes: 'open' } });
    expect(poseFor('holdSign', 0, { breathe: false })).toMatchObject({ motion: null, parts: { item: true } });
  });

  it('the still idle blink shuts the eyes for 150 ms with no animation', () => {
    expect(poseFor('fidget', 4)).toMatchObject({ motion: 'shut', ms: 150, parts: { eyes: 'open' } });
    expect(poseFor('fidget', 0)).toMatchObject({ motion: 'blink', ms: 1200 });
  });
});
