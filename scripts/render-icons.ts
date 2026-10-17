// Renders the app icon from Kitty's pixel art (src/pet/pixelCat.ts, peach skin) and the
// menu-bar template icon from the Cat.dc.html soft-clay cat's head silhouette.
import { Resvg } from '@resvg/resvg-js';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { SKINS } from '../src/pet/catalog';
import { COLS, EYES, OUTLINE, PIXEL_MAP, ROWS } from '../src/pet/pixelCat';

const root = join(dirname(new URL(import.meta.url).pathname), '..');
const out = join(root, 'src-tauri/icons');
mkdirSync(out, { recursive: true });

// 40 px cells: the 22×17 cat is 880×680, centred on the 1024 canvas so every edge lands on a
// whole pixel.
const CELL = 40;
const { palette } = SKINS.peach;
const fills: Record<string, string> = { ...palette, o: OUTLINE };
const cell = (x: number, y: number, fill: string) =>
  `<rect x="${x * CELL}" y="${y * CELL}" width="${CELL}" height="${CELL}" fill="${fill}"/>`;
const cells = PIXEL_MAP.flatMap((row, y) => [...row].flatMap((ch, x) => (fills[ch] ? [cell(x, y, fills[ch])] : [])));
// Open eyes: the iris on the pupil.
const eyes = EYES.flatMap(([x, y]) => [cell(x, y, palette.e), cell(x, y + 1, OUTLINE)]);
const appSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024" shape-rendering="crispEdges"><g transform="translate(${(1024 - COLS * CELL) / 2} ${(1024 - ROWS * CELL) / 2})">${cells.join('')}${eyes.join('')}</g></svg>`;
writeFileSync(
  join(out, 'app-icon.png'),
  new Resvg(appSvg, { fitTo: { mode: 'width', value: 1024 }, background: 'rgba(0,0,0,0)' }).render().asPng(),
);

// Head silhouette: head circle cx60 cy50 r34 plus both ears. Bounds x 26..94, y 4..84 → 68×80.
const head = `
<g fill="#000000">
  <polygon points="30,32 28,4 54,20"/>
  <polygon points="90,32 92,4 66,20"/>
  <circle cx="60" cy="50" r="34"/>
</g>`;
const traySvg = `<svg xmlns="http://www.w3.org/2000/svg" width="44" height="44" viewBox="16 2 88 88">${head}</svg>`;
writeFileSync(
  join(out, 'tray-template.png'),
  new Resvg(traySvg, { fitTo: { mode: 'width', value: 44 }, background: 'rgba(0,0,0,0)' }).render().asPng(),
);
console.log('icons written to', out);
