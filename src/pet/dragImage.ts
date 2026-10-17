// Drag-out preview (plan Step 7.2): a 280×64 card drawn on a 2× canvas, rotated
// 1.5°, with the kind icon (from ICONS via Path2D), the title in `label` style
// and, for a multi-selection, a count badge. Returned as a PNG data URL, which
// tauri-plugin-drag accepts as `icon`.
import { ICONS, KIND_ICON, type IconName } from '../ds';
import { displayKind } from '../shared/itemKind';
import type { ItemKind } from '../shared/types';

const CARD_W = 280;
const CARD_H = 64;
/** room around the card for the rotation and the shadow */
const PAD = 24;
const SCALE = 2;
const ROTATE_DEG = 1.5;

export interface DragImageInput {
  kind: ItemKind;
  /** a meeting's item draws the meeting icon */
  meetingId: string | null;
  title: string;
  /** number of dragged items; a badge shows when > 1 */
  count: number;
}

export function renderDragImage(input: DragImageInput): string {
  const canvas = document.createElement('canvas');
  canvas.width = (CARD_W + PAD * 2) * SCALE;
  canvas.height = (CARD_H + PAD * 2) * SCALE;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('canvas 2d unavailable');
  const css = getComputedStyle(document.documentElement);
  const v = (name: string) => css.getPropertyValue(name).trim();

  ctx.scale(SCALE, SCALE);
  ctx.translate(PAD + CARD_W / 2, PAD + CARD_H / 2);
  ctx.rotate((ROTATE_DEG * Math.PI) / 180);
  ctx.translate(-CARD_W / 2, -CARD_H / 2);

  // Card: `bubble` fill, `bubble-ring` edge, `shadow-drag` approximation.
  ctx.save();
  ctx.shadowColor = 'rgba(42, 33, 28, 0.24)';
  ctx.shadowBlur = 16;
  ctx.shadowOffsetY = 6;
  ctx.beginPath();
  ctx.roundRect(0, 0, CARD_W, CARD_H, 12);
  ctx.fillStyle = v('--bubble');
  ctx.fill();
  ctx.restore();
  ctx.lineWidth = 1;
  ctx.strokeStyle = v('--bubble-ring');
  ctx.beginPath();
  ctx.roundRect(0.5, 0.5, CARD_W - 1, CARD_H - 1, 12);
  ctx.stroke();

  // Kind tile with the icon.
  const tile = 40;
  const tx = 12;
  const ty = (CARD_H - tile) / 2;
  ctx.beginPath();
  ctx.roundRect(tx, ty, tile, tile, 8);
  ctx.fillStyle = v('--surface-sunken');
  ctx.fill();
  drawIcon(ctx, KIND_ICON[displayKind(input)] ?? 'file', tx + (tile - 20) / 2, ty + (tile - 20) / 2, 20, v('--ink-muted'));

  // Title in `label` style (500 13px/20px sans), one line with an ellipsis.
  const textX = tx + tile + 12;
  const badgeRoom = input.count > 1 ? 36 : 0;
  ctx.font = `500 13px ${v('--font-sans')}`;
  ctx.fillStyle = v('--bubble-ink');
  ctx.textBaseline = 'middle';
  ctx.fillText(fit(ctx, input.title, CARD_W - textX - 12 - badgeRoom), textX, CARD_H / 2);

  if (input.count > 1) {
    const label = input.count > 99 ? '99+' : String(input.count);
    ctx.font = `700 11px ${v('--font-sans')}`;
    const w = Math.max(20, ctx.measureText(label).width + 10);
    const bx = CARD_W - 12 - w;
    const by = (CARD_H - 20) / 2;
    ctx.beginPath();
    ctx.roundRect(bx, by, w, 20, 10);
    ctx.fillStyle = v('--badge');
    ctx.fill();
    ctx.fillStyle = v('--on-badge');
    ctx.textAlign = 'center';
    ctx.fillText(label, bx + w / 2, CARD_H / 2);
  }

  const png = dataUrlBytes(canvas.toDataURL('image/png'));
  // AppKit sizes the drag image from the PNG's DPI: mark it 144 dpi so the 2× pixels show at 1×.
  return `data:image/png;base64,${toBase64(pngWithDpi(png, 72 * SCALE))}`;
}

function fit(ctx: CanvasRenderingContext2D, text: string, max: number): string {
  const line = text.replace(/\s+/g, ' ').trim();
  if (ctx.measureText(line).width <= max) return line;
  let lo = 0;
  let hi = line.length;
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2);
    if (ctx.measureText(line.slice(0, mid) + '…').width <= max) lo = mid;
    else hi = mid - 1;
  }
  return line.slice(0, lo).trimEnd() + '…';
}

/** Strokes a design-system icon (24-unit viewBox, 1.75 stroke, round caps) at `size` px. */
function drawIcon(ctx: CanvasRenderingContext2D, name: IconName, x: number, y: number, size: number, color: string) {
  ctx.save();
  ctx.translate(x, y);
  ctx.scale(size / 24, size / 24);
  ctx.strokeStyle = color;
  ctx.lineWidth = 1.75;
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';
  for (const path of iconPaths(ICONS[name])) ctx.stroke(path);
  ctx.restore();
}

/** Turns the icon's `<path>`, `<circle>` and `<rect>` markup into Path2D objects. */
function iconPaths(markup: string): Path2D[] {
  const out: Path2D[] = [];
  for (const m of markup.matchAll(/<(path|circle|rect)\s([^>]*?)\/?>/g)) {
    const attrs = Object.fromEntries([...m[2].matchAll(/([\w-]+)="([^"]*)"/g)].map((a) => [a[1], a[2]]));
    const n = (k: string) => Number(attrs[k] ?? 0);
    if (m[1] === 'path') {
      out.push(new Path2D(attrs.d));
    } else if (m[1] === 'circle') {
      const p = new Path2D();
      p.arc(n('cx'), n('cy'), n('r'), 0, Math.PI * 2);
      out.push(p);
    } else {
      const p = new Path2D();
      p.roundRect(n('x'), n('y'), n('width'), n('height'), n('rx'));
      out.push(p);
    }
  }
  return out;
}

function dataUrlBytes(url: string): Uint8Array {
  const bin = atob(url.slice(url.indexOf(',') + 1));
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

function toBase64(bytes: Uint8Array): string {
  let bin = '';
  for (let i = 0; i < bytes.length; i += 0x8000) {
    bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(bin);
}

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

export function crc32(bytes: Uint8Array): number {
  let c = 0xffffffff;
  for (const b of bytes) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

/**
 * Inserts (or replaces) the PNG `pHYs` chunk so the image declares `dpi`.
 * The chunk goes right after IHDR (signature 8 bytes + IHDR 25 bytes).
 */
export function pngWithDpi(png: Uint8Array, dpi: number): Uint8Array {
  const ppm = Math.round(dpi / 0.0254);
  const chunk = new Uint8Array(21);
  const view = new DataView(chunk.buffer);
  view.setUint32(0, 9);
  chunk.set([0x70, 0x48, 0x59, 0x73], 4); // "pHYs"
  view.setUint32(8, ppm);
  view.setUint32(12, ppm);
  chunk[16] = 1; // unit: metre
  view.setUint32(17, crc32(chunk.subarray(4, 17)));

  const head = 8 + 25;
  const parts: Uint8Array[] = [png.subarray(0, head), chunk];
  // Copy the remaining chunks, dropping any existing pHYs.
  const src = new DataView(png.buffer, png.byteOffset, png.byteLength);
  let at = head;
  while (at + 8 <= png.length) {
    const len = src.getUint32(at);
    const end = at + 12 + len;
    const type = String.fromCharCode(...png.subarray(at + 4, at + 8));
    if (type !== 'pHYs') parts.push(png.subarray(at, end));
    at = end;
  }
  const out = new Uint8Array(parts.reduce((s, p) => s + p.length, 0));
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}
