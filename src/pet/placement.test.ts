import { describe, expect, it } from 'vitest';
import { placePanel, snapToEdge } from './placement';

const area = { w: 1440, h: 875 };
const panel = { w: 360, h: 400 };

describe('placePanel', () => {
  it('opens left of a pet on the right edge, centred on it', () => {
    const pet = { x: 1396, y: 480, w: 44, h: 80 };
    expect(placePanel('right', pet, panel, area)).toEqual({ x: 1396 - 24 - 360, y: 520 - 200 });
  });

  it('opens right of a pet on the left edge, centred on it', () => {
    const pet = { x: 0, y: 300, w: 44, h: 80 };
    expect(placePanel('left', pet, panel, area)).toEqual({ x: 44 + 24, y: 340 - 200 });
  });

  it('opens above a pet on the bottom edge, centred on it', () => {
    const pet = { x: 700, y: 831, w: 80, h: 44 };
    expect(placePanel('bottom', pet, panel, area)).toEqual({ x: 740 - 180, y: 831 - 24 - 400 });
  });

  it('clamps 16 px inside the work area at a corner', () => {
    // Pet at the bottom of the right edge: the centred panel would overflow the bottom.
    expect(placePanel('right', { x: 1396, y: 820, w: 44, h: 55 }, panel, area))
      .toEqual({ x: 1012, y: 875 - 16 - 400 });
    // Pet at the left end of the bottom edge: the centred panel would overflow the left.
    expect(placePanel('bottom', { x: 0, y: 831, w: 80, h: 44 }, panel, area))
      .toEqual({ x: 16, y: 407 });
  });
});

describe('snapToEdge', () => {
  it('snaps to the nearest edge with the offset along it', () => {
    expect(snapToEdge({ x: 1400, y: 437.5 }, area, 80)).toEqual({ edge: 'right', offset: 0.5 });
    expect(snapToEdge({ x: 30, y: 175 }, area, 80)).toEqual({ edge: 'left', offset: 0.2 });
    expect(snapToEdge({ x: 720, y: 860 }, area, 80)).toEqual({ edge: 'bottom', offset: 0.5 });
  });

  it('keeps the whole pet on the edge near a corner', () => {
    expect(snapToEdge({ x: 1435, y: 5 }, area, 80)).toEqual({ edge: 'right', offset: 40 / 875 });
  });
});
