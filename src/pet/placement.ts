// Pure geometry for the pet window (plan Step 5.3). The pet window covers the
// monitor's work area, so every coordinate here is window-local CSS px.
import type { Edge } from '../shared/types';

export interface Point { x: number; y: number }
export interface Size { w: number; h: number }
export interface Box { x: number; y: number; w: number; h: number }

/** Gap between the pet and a panel opening away from the edge. */
export const PANEL_GAP = 24;
/** Panels stay this far inside the work area. */
export const PANEL_MARGIN = 16;
/** The PetDock box: a square centred on the edge point, one side on the edge. */
export const DOCK_BOX = 360;
/** Share of the pet past the screen edge while peeking (`.cp-pet` translate). */
export const PEEK_HIDDEN = 0.45;
/** The top of the art is the FX zone and never takes the pointer. */
export const FX_ZONE = 0.22;
/** PetDock's default bubble arc radius and the `bubble-size` token. */
export const BUBBLE_ARC = 100;
export const BUBBLE_SIZE = 40;

/** The point on the edge the pet is anchored to; `offset` runs 0..1 along the edge. */
export function edgePoint(edge: Edge, offset: number, area: Size): Point {
  switch (edge) {
    case 'left':
      return { x: 0, y: offset * area.h };
    case 'right':
      return { x: area.w, y: offset * area.h };
    case 'bottom':
      return { x: offset * area.w, y: area.h };
  }
}

/** Top-left of the DOCK_BOX square for the pet docked at `p`. */
export function dockBox(edge: Edge, p: Point, area: Size): Point {
  const half = DOCK_BOX / 2;
  switch (edge) {
    case 'left':
      return { x: 0, y: p.y - half };
    case 'right':
      return { x: area.w - DOCK_BOX, y: p.y - half };
    case 'bottom':
      return { x: p.x - half, y: area.h - DOCK_BOX };
  }
}

/** The pet's full square when slid out (panels are placed against it). */
export function petBox(edge: Edge, p: Point, size: number): Box {
  switch (edge) {
    case 'left':
      return { x: 0, y: p.y - size / 2, w: size, h: size };
    case 'right':
      return { x: p.x - size, y: p.y - size / 2, w: size, h: size };
    case 'bottom':
      return { x: p.x - size / 2, y: p.y - size, w: size, h: size };
  }
}

/**
 * The pointer-taking part of the pet: the on-screen part of the art (all of it
 * when out, 55% when peeking) minus the top FX zone.
 */
export function petHitRect(edge: Edge, p: Point, size: number, out: boolean): Box {
  const box = petBox(edge, p, size);
  const shown = out ? size : size * (1 - PEEK_HIDDEN);
  const fx = size * FX_ZONE;
  switch (edge) {
    case 'left':
      return { x: 0, y: box.y + fx, w: shown, h: size - fx };
    case 'right':
      return { x: p.x - shown, y: box.y + fx, w: shown, h: size - fx };
    case 'bottom': {
      const top = p.y - shown + fx;
      return { x: box.x, y: top, w: size, h: Math.max(0, p.y - top) };
    }
  }
}

/** Bubble squares fanned across 140° away from the edge (same math as PetDock). */
export function bubbleRects(edge: Edge, p: Point, size: number, n: number): Box[] {
  const box = petBox(edge, p, size);
  const cx = box.x + size / 2;
  const cy = box.y + size / 2;
  const base = { right: 180, left: 0, bottom: 270 }[edge];
  const spread = n > 1 ? 140 : 0;
  return Array.from({ length: n }, (_, i) => {
    const a = ((base - spread / 2 + (n > 1 ? (spread * i) / (n - 1) : 0)) * Math.PI) / 180;
    let x = Math.cos(a) * BUBBLE_ARC;
    let y = -Math.sin(a) * BUBBLE_ARC;
    if (edge === 'bottom') {
      x = Math.cos(a + Math.PI) * BUBBLE_ARC;
      y = Math.sin(a) * BUBBLE_ARC;
    }
    const h = BUBBLE_SIZE / 2;
    return { x: cx + x - h, y: cy + y - h, w: BUBBLE_SIZE, h: BUBBLE_SIZE };
  });
}

/** Top-left of a menu opened at the pointer: it extends away from the edge, clamped inside. */
export function placeAtPointer(edge: Edge, pointer: Point, menu: Size, area: Size): Point {
  const x = edge === 'right' ? pointer.x - menu.w : edge === 'left' ? pointer.x : pointer.x - menu.w / 2;
  const y = edge === 'bottom' ? pointer.y - menu.h : pointer.y;
  return {
    x: clamp(x, PANEL_MARGIN, area.w - PANEL_MARGIN - menu.w),
    y: clamp(y, PANEL_MARGIN, area.h - PANEL_MARGIN - menu.h),
  };
}

/**
 * Top-left of a panel that opens away from the edge, `PANEL_GAP` beyond the
 * pet, centred on it, clamped `PANEL_MARGIN` inside the work area.
 */
export function placePanel(edge: Edge, pet: Box, panel: Size, area: Size): Point {
  const cx = pet.x + pet.w / 2;
  const cy = pet.y + pet.h / 2;
  let x: number;
  let y: number;
  switch (edge) {
    case 'left':
      x = pet.x + pet.w + PANEL_GAP;
      y = cy - panel.h / 2;
      break;
    case 'right':
      x = pet.x - PANEL_GAP - panel.w;
      y = cy - panel.h / 2;
      break;
    case 'bottom':
      x = cx - panel.w / 2;
      y = pet.y - PANEL_GAP - panel.h;
      break;
  }
  return {
    x: clamp(x, PANEL_MARGIN, area.w - PANEL_MARGIN - panel.w),
    y: clamp(y, PANEL_MARGIN, area.h - PANEL_MARGIN - panel.h),
  };
}

/**
 * Where a dragged pet lands on release: the nearest of the left, right and
 * bottom edges, and its offset along that edge, kept so the whole pet
 * (`petSize` px) stays on the edge.
 */
export function snapToEdge(p: Point, area: Size, petSize: number): { edge: Edge; offset: number } {
  const distances: [Edge, number][] = [
    ['left', p.x],
    ['right', area.w - p.x],
    ['bottom', area.h - p.y],
  ];
  const [edge] = distances.reduce((best, d) => (d[1] < best[1] ? d : best));
  const length = edge === 'bottom' ? area.w : area.h;
  const along = edge === 'bottom' ? p.x : p.y;
  const half = petSize / 2;
  return { edge, offset: clamp(along, half, length - half) / length };
}

/** Lower bound wins when the range is empty (panel larger than the area). */
function clamp(v: number, min: number, max: number): number {
  return Math.max(min, Math.min(v, max));
}
