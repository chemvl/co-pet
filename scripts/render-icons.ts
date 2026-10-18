// Renders the app icon from Kitty's pixel art (src/pet/pixelCat.ts, peach skin), the macOS
// menu-bar template icon and the Windows tray icon from the Cat.dc.html soft-clay cat's head
// silhouette.
import { Resvg } from '@resvg/resvg-js';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { SKINS } from '../src/pet/catalog';
import { COLS, EYES, OUTLINE, PIXEL_MAP, ROWS } from '../src/pet/pixelCat';

const root = join(import.meta.dir, '..');
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
const renderHead = (file: string, fill: string, size: number) => {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="16 2 88 88">
<g fill="${fill}">
  <polygon points="30,32 28,4 54,20"/>
  <polygon points="90,32 92,4 66,20"/>
  <circle cx="60" cy="50" r="34"/>
</g></svg>`;
  writeFileSync(
    join(out, file),
    new Resvg(svg, { fitTo: { mode: 'width', value: size }, background: 'rgba(0,0,0,0)' }).render().asPng(),
  );
};
// macOS tints the template; the Windows tray shows the colors as drawn.
renderHead('tray-template.png', '#000000', 44);
renderHead('tray.png', '#1f2933', 32);
console.log('icons written to', out);
