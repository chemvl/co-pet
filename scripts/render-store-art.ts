// Renders the Microsoft Store listing images (Partner Center › Store listing) from Kitty's pixel
// art (src/pet/pixelCat.ts, peach skin) into docs/store/:
// - poster art 9:16 (720×1080, 1440×2160) and box art 1:1 (1080×1080, 2160×2160): Kitty and the
//   "Co-pet" wordmark in Baloo 2 on peach-soft. Microsoft overlays text on the bottom quarter of
//   poster art, so the title sits in the top three quarters.
// - super hero art 16:9 (1920×1080, 3840×2160): Kitty on peach-soft with no title, as the Store
//   requires. The Store lays the listing's title and buttons over the left side, so Kitty sits right.
// - store display images (300×300, 150×150, 71×71): Kitty on transparent, like the package logos.
// Every cell is a whole number of pixels so the art stays crisp.
import { Resvg } from '@resvg/resvg-js';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { inflateSync } from 'node:zlib';
import { SKINS } from '../src/pet/catalog';
import { COLS, EYES, OUTLINE, PIXEL_MAP, ROWS } from '../src/pet/pixelCat';

const root = join(import.meta.dir, '..');
const out = join(root, 'docs/store');
mkdirSync(out, { recursive: true });

// docs/design/design-system/tokens.css, light theme.
const PEACH_SOFT = '#fde8dc';
const INK = '#2a211c';

// resvg loads TTF/OTF only; @fontsource ships WOFF, so unwrap WOFF 1.0 into a plain sfnt.
const woffToSfnt = (woff: Buffer): Buffer => {
  const numTables = woff.readUInt16BE(12);
  const tables = Array.from({ length: numTables }, (_, i) => {
    const at = 44 + i * 20;
    const offset = woff.readUInt32BE(at + 4);
    const compLength = woff.readUInt32BE(at + 8);
    const origLength = woff.readUInt32BE(at + 12);
    const raw = woff.subarray(offset, offset + compLength);
    return {
      tag: woff.subarray(at, at + 4),
      checksum: woff.readUInt32BE(at + 16),
      data: compLength < origLength ? inflateSync(raw) : raw,
    };
  });
  const entrySelector = Math.floor(Math.log2(numTables));
  const searchRange = 2 ** entrySelector * 16;
  const head = Buffer.alloc(12 + numTables * 16);
  woff.copy(head, 0, 4, 8); // flavor
  head.writeUInt16BE(numTables, 4);
  head.writeUInt16BE(searchRange, 6);
  head.writeUInt16BE(entrySelector, 8);
  head.writeUInt16BE(numTables * 16 - searchRange, 10);
  const bodies: Buffer[] = [];
  let offset = head.length;
  tables.forEach((t, i) => {
    const at = 12 + i * 16;
    t.tag.copy(head, at);
    head.writeUInt32BE(t.checksum, at + 4);
    head.writeUInt32BE(offset, at + 8);
    head.writeUInt32BE(t.data.length, at + 12);
    const padded = Buffer.alloc((t.data.length + 3) & ~3);
    t.data.copy(padded);
    bodies.push(padded);
    offset += padded.length;
  });
  return Buffer.concat([head, ...bodies]);
};

const fontDir = mkdtempSync(join(tmpdir(), 'copet-store-'));
const fontFile = join(fontDir, 'baloo-2-700.ttf');
writeFileSync(
  fontFile,
  woffToSfnt(readFileSync(join(root, 'node_modules/@fontsource/baloo-2/files/baloo-2-latin-700-normal.woff'))),
);

const { palette } = SKINS.peach;
const fills: Record<string, string> = { ...palette, o: OUTLINE };
// Kitty with `cell`-px cells, top-left at (x, y).
const kitty = (cell: number, x: number, y: number) => {
  const rect = (cx: number, cy: number, fill: string) =>
    `<rect x="${cx * cell}" y="${cy * cell}" width="${cell}" height="${cell}" fill="${fill}"/>`;
  const cells = PIXEL_MAP.flatMap((row, cy) => [...row].flatMap((ch, cx) => (fills[ch] ? [rect(cx, cy, fills[ch])] : [])));
  // Open eyes: the iris on the pupil.
  const eyes = EYES.flatMap(([cx, cy]) => [rect(cx, cy, palette.e), rect(cx, cy + 1, OUTLINE)]);
  return `<g transform="translate(${x} ${y})" shape-rendering="crispEdges">${cells.join('')}${eyes.join('')}</g>`;
};

const wordmark = (x: number, baseline: number, size: number) =>
  `<text x="${x}" y="${baseline}" text-anchor="middle" font-family="Baloo 2" font-weight="700" font-size="${size}" fill="${INK}">Co-pet</text>`;

const render = (file: string, w: number, h: number, body: string, scale = 1, ground?: string) => {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">${
    ground ? `<rect width="${w}" height="${h}" fill="${ground}"/>` : ''
  }${body}</svg>`;
  const png = new Resvg(svg, {
    fitTo: { mode: 'width', value: w * scale },
    background: 'rgba(0,0,0,0)',
    font: { loadSystemFonts: false, fontFiles: [fontFile], defaultFontFamily: 'Baloo 2' },
  })
    .render()
    .asPng();
  writeFileSync(join(out, file), png);
};

// Poster art 9:16. 24 px cells (528×408); wordmark baseline at 790 of 1080, above the bottom quarter (810).
const poster = `${kitty(24, (720 - COLS * 24) / 2, 250)}${wordmark(360, 790, 136)}`;
render('poster-720x1080.png', 720, 1080, poster, 1, PEACH_SOFT);
render('poster-1440x2160.png', 720, 1080, poster, 2, PEACH_SOFT);

// Box art 1:1. 30 px cells (660×510).
const box = `${kitty(30, (1080 - COLS * 30) / 2, 170)}${wordmark(540, 900, 160)}`;
render('box-1080x1080.png', 1080, 1080, box, 1, PEACH_SOFT);
render('box-2160x2160.png', 1080, 1080, box, 2, PEACH_SOFT);

// Super hero art 16:9. 36 px cells (792×612), 200 px from the right edge, vertically centred.
const hero = kitty(36, 1920 - COLS * 36 - 200, (1080 - ROWS * 36) / 2);
render('hero-1920x1080.png', 1920, 1080, hero, 1, PEACH_SOFT);
render('hero-3840x2160.png', 1920, 1080, hero, 2, PEACH_SOFT);

// Store display images: Kitty centred on transparent at the largest whole cell that leaves a margin.
for (const [size, cell] of [
  [300, 12],
  [150, 6],
  [71, 3],
] as const) {
  render(
    `store-${size}x${size}.png`,
    size,
    size,
    kitty(cell, Math.floor((size - COLS * cell) / 2), Math.floor((size - ROWS * cell) / 2)),
  );
}

rmSync(fontDir, { recursive: true, force: true });
console.log('store art written to', out);
