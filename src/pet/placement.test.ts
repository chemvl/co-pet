import { describe, expect, it } from 'vitest';
import { bubbleRects, petHitRect, placePanel, snapToEdge } from './placement';

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

describe('petHitRect', () => {
  const rounded = (r: { x: number; y: number; w: number; h: number }) =>
    ({ x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.w), h: Math.round(r.h) });

  it('takes only the on-screen 55% while peeking, minus the top 22% of the art', () => {
    expect(rounded(petHitRect('right', { x: 1440, y: 400 }, 100, false))).toEqual({ x: 1385, y: 372, w: 55, h: 78 });
    expect(rounded(petHitRect('left', { x: 0, y: 400 }, 100, false))).toEqual({ x: 0, y: 372, w: 55, h: 78 });
    // Bottom: the visible top 55 px, of which the top 22 px are the FX zone.
    expect(rounded(petHitRect('bottom', { x: 700, y: 875 }, 100, false))).toEqual({ x: 650, y: 842, w: 100, h: 33 });
  });

  it('takes the whole art below the FX zone when out', () => {
    expect(rounded(petHitRect('right', { x: 1440, y: 400 }, 100, true))).toEqual({ x: 1340, y: 372, w: 100, h: 78 });
  });
});

describe('bubbleRects', () => {
  it('fans away from the right edge, symmetric about the pet centre', () => {
    const rects = bubbleRects('right', { x: 1440, y: 400 }, 80, 4);
    const centres = rects.map((r) => ({ x: r.x + r.w / 2, y: r.y + r.h / 2 }));
    // Pet centre (1400, 400); every bubble is 100 px from it and left of it.
    for (const c of centres) {
      expect(Math.hypot(c.x - 1400, c.y - 400)).toBeCloseTo(100);
      expect(c.x).toBeLessThan(1400);
    }
    expect(centres[0].y + centres[3].y).toBeCloseTo(800);
  });

  it('fans upward from the bottom edge', () => {
    for (const r of bubbleRects('bottom', { x: 700, y: 875 }, 80, 4)) expect(r.y + r.h / 2).toBeLessThan(835);
  });
});
