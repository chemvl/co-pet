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
