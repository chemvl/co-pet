// Pixel pets: a 16×16 grid of 7.5-unit cells in the same 120×120 body box as the cat
// (feet on the bottom edge, top 22% free for FX). Rows are palette keys, '.' is empty,
// and a row may run two cells past the box on the right. Eyes and the mouth are overlays,
// so every pose from pose.ts works. The `tail` and `ear` layers carry the cat's fidget
// classes, so the tail and ear fidgets move them. The palette is the species' fixed keys plus
// the skin's (catalog.ts).
import type { ReactElement } from 'react';
import { defaultSkin, PIXEL_SPECIES, skinsOf, type PixelSpecies, type SkinId } from './catalog';
import type { CatEyes, CatParts } from './pose';

const CELL = 7.5;
const INK = '#2a211c';
const GLINT = '#ffffff';
const PAPER_LINE = '#8f8170';
const MOUTH = '#8c3a2b';

/** x, y, width, height in cells, then a palette key */
type Cell = readonly [number, number, number, number, string];

interface PixelPet {
  /** the keys that don't change with the skin */
  palette: Readonly<Record<string, string>>;
  body: readonly string[];
  /** drawn behind the body; the tail fidget wags it */
  tail: readonly string[];
  /** drawn in front of the body; the ear fidget twitches it */
  ear: readonly string[];
  /** top-left cell of each 2×2 eye, viewer's left first */
  eyes: readonly [readonly [number, number], readonly [number, number]];
  /** drawn only while the mouth is closed */
  smile: readonly Cell[];
  open: readonly Cell[];
  /** top-left cell of the held note; `hand` is the palette key of the paws holding it */
  item: readonly [number, number];
  hand: string;
}

const PETS: Readonly<Record<PixelSpecies, PixelPet>> = {
  chicken: {
    palette: { o: INK, k: '#ffb0a8', d: MOUTH },
    body: [
      '',
      '',
      '',
      '...oooooooooo...',
      '..owwwwwwwwwwo..',
      '.owwwwwwwwwwwwo.',
      '.owwwwwwwwwwwwo.',
      '.owwwwwwwwwwwwo.',
      '.owwwwwwwwwwwwo.',
      '.owkkwwyywkkwwo.',
      '.osSwwwwrwwwwwo.',
      '.ossSwwwwwwwwwo.',
      '.ossSwwwwwwwsso.',
      '..ossswwwwssso..',
      '...oooooooooo...',
      '....yyy..yyy....',
    ],
    // A chicken shows no tail from the front; the comb wiggles instead.
    tail: [
      '.......oo.......',
      '.....oorroo.....',
      '....orrrrrro....',
    ],
    ear: [
      '', '', '', '', '', '', '', '', '', '',
      '............Sso.',
      '...........Ssso.',
      '...........Ssso.',
    ],
    eyes: [[4, 7], [9, 7]],
    smile: [],
    open: [[7, 9.5, 2, 0.5, 'd'], [7, 10, 2, 0.5, 'y']],
    item: [6.25, 10.5],
    hand: 's',
  },
  dog: {
    palette: { o: INK, k: '#f5a08a', g: '#ffd25a', d: MOUTH, t: '#ff7a8a' },
    body: [
      '....oooooooo....',
      '.oooobbbbbbbo...',
      'oeeeobbbbbbbbo..',
      'oeeeobbbbbbbbbo.',
      'oeeeobbbbbbbbbo.',
      '.oeeobbbbbbbbbo.',
      '.ooobbbbbbbbbbo.',
      '.obbbbbbbbbbbbo.',
      '.obbbbbbbbbbbbo.',
      '.okkbmmoommbkko.',
      '.obbbmmmmmmbbbo.',
      '..occccggcccco..',
      '..obbbmggmbbbo..',
      '..obbbmmmmbbbo..',
      '...oooooooooo...',
      '...obbo..obbo...',
    ],
    tail: [
      '', '', '', '', '', '', '', '',
      '................o.',
      '...............obo',
      '..............obbo',
      '..............ooo.',
    ],
    ear: [
      '',
      '...........oooo.',
      '...........oeeeo',
      '...........oeeeo',
      '...........oeeeo',
      '...........oeeo.',
      '............oo..',
    ],
    eyes: [[4, 7], [10, 7]],
    // A little "w" under the nose.
    smile: [
      [6.5, 10, 0.5, 0.5, 'o'], [7, 10.5, 0.5, 0.5, 'o'], [7.5, 10, 0.5, 0.5, 'o'],
      [8, 10.5, 0.5, 0.5, 'o'], [8.5, 10, 0.5, 0.5, 'o'],
    ],
    open: [[6.5, 10, 2, 1, 'd'], [7, 10.5, 1, 0.75, 't']],
    item: [6.25, 11],
    hand: 'b',
  },
  pig: {
    palette: { o: INK, k: '#ff8aa3', d: MOUTH, t: '#ff6f8a' },
    body: [
      '................',
      '................',
      '..oo............',
      '..oPo...........',
      '..oPPooooo......',
      '.oppppppppppppo.',
      '.oppppppppppppo.',
      '.oppppppppppppo.',
      '.okkppPPPPppkko.',
      '.opppPxnnxPpppo.',
      '.opppPnnnnPpppo.',
      '.oppppPPPPppppo.',
      '.oppppppppppppo.',
      '..oPppppppppPo..',
      '...oooooooooo...',
      '...oPPo..oPPo...',
    ],
    tail: [
      '', '', '', '', '', '', '', '', '',
      '................xx',
      '...............x.x',
      '...............xx.',
    ],
    ear: [
      '', '',
      '............oo..',
      '...........oPo..',
      '..........oPPo..',
    ],
    eyes: [[4, 6], [9, 6]],
    smile: [],
    open: [[7, 12, 2, 1, 'd'], [7, 12.5, 2, 0.5, 't']],
    item: [6.25, 11.25],
    hand: 'p',
  },
  owl: {
    palette: { o: INK, k: '#f4a08c', d: MOUTH },
    body: [
      '..o.............',
      '..oBo...........',
      '..oBBoooooo.....',
      '.obbbbbbbbbbbbo.',
      '.obbffbbbbffbbo.',
      '.obffffbbffffbo.',
      '.obffffbbffffbo.',
      '.obbffbyybffbbo.',
      '.okkbbbyybbbkko.',
      '.obbbllllllbbbo.',
      '.oBBlvllllvlbbo.',
      '.oBBllllllllbbo.',
      '.oBBllvllvllbbo.',
      '..oBBllllllbbo..',
      '...oooooooooo...',
      '....yyy..yyy....',
    ],
    tail: [
      '.............o..',
      '...........oBo..',
      '...........BBo..',
    ],
    ear: [
      '', '', '', '', '', '', '', '', '', '',
      '............BB..',
      '............BB..',
      '............BB..',
      '...........BB...',
    ],
    eyes: [[4, 5], [10, 5]],
    smile: [],
    open: [[7, 8, 2, 0.5, 'd'], [7, 8.5, 2, 0.5, 'y']],
    item: [6.25, 10],
    hand: 'B',
  },
};

type Run = readonly [x: number, y: number, w: number, fill: string];

/** Same-colour cells on a row merge into one rect. */
function runs(rows: readonly string[], palette: Readonly<Record<string, string>>): Run[] {
  const out: Run[] = [];
  rows.forEach((row, y) => {
    let x = 0;
    while (x < row.length) {
      const ch = row[x];
      let end = x + 1;
      while (end < row.length && row[end] === ch) end++;
      const fill = palette[ch];
      if (fill) out.push([x, y, end - x, fill]);
      x = end;
    }
  });
  return out;
}

interface Layers { palette: Readonly<Record<string, string>>; body: Run[]; tail: Run[]; ear: Run[] }

const LAYERS: Partial<Record<SkinId, Layers>> = Object.fromEntries(
  PIXEL_SPECIES.flatMap((s) =>
    skinsOf(s).map((k) => {
      const palette = { ...PETS[s].palette, ...k.palette };
      return [k.id, { palette, body: runs(PETS[s].body, palette), tail: runs(PETS[s].tail, palette), ear: runs(PETS[s].ear, palette) }];
    }),
  ),
);

/** Eye shapes in cells, relative to the eye's top-left. */
type Shape = readonly (readonly [number, number, number, number])[];
const OPEN: Shape = [[0, -0.5, 2, 2.5]];
const SHUT: Shape = [[0, 0.5, 0.5, 0.5], [0.5, 1, 1, 0.5], [1.5, 0.5, 0.5, 0.5]];
const EYES: Readonly<Record<CatEyes, { ink: Shape; glint: Shape }>> = {
  open: { ink: OPEN, glint: [[1, 0, 0.5, 0.5]] },
  wide: { ink: [[0, -1, 2, 3]], glint: [[1, -0.5, 0.5, 0.5], [0.5, 1, 0.5, 0.5]] },
  up: { ink: [[0, -1, 2, 2]], glint: [] },
  closed: { ink: SHUT, glint: [] },
  happy: { ink: [[0, 1, 0.5, 0.5], [0.5, 0.5, 1, 0.5], [1.5, 1, 0.5, 0.5]], glint: [] },
  // Handled per eye below.
  wink: { ink: OPEN, glint: [] },
};

function cellRect(key: string | number, x: number, y: number, w: number, h: number, fill: string): ReactElement {
  return <rect key={key} x={x * CELL} y={y * CELL} width={w * CELL} height={h * CELL} fill={fill} />;
}

function runRects(list: readonly Run[]) {
  return list.map(([x, y, w, fill], i) => cellRect(i, x, y, w, 1, fill));
}

function eyeRects(pet: PixelPet, shapes: readonly [Shape, Shape], fill: string) {
  return pet.eyes.flatMap(([ex, ey], e) =>
    shapes[e].map(([x, y, w, h], i) => cellRect(`${e}-${i}`, ex + x, ey + y, w, h, fill)),
  );
}

export function PixelPet(p: { species: PixelSpecies; skin: SkinId; parts: CatParts; lowered: boolean; blink: boolean }) {
  const pet = PETS[p.species];
  const layers = LAYERS[p.skin] ?? LAYERS[defaultSkin(p.species)]!;
  const { eyes, mouth, item } = p.parts;
  const ink: [Shape, Shape] =
    eyes === 'wink' ? [EYES.open.ink, SHUT] : [EYES[eyes].ink, EYES[eyes].ink];
  const glint: [Shape, Shape] =
    eyes === 'wink' ? [EYES.open.glint, []] : [EYES[eyes].glint, EYES[eyes].glint];
  const [ix, iy] = pet.item;

  return (
    <g transform={`translate(20 ${p.lowered ? 44 : 40})`} shapeRendering="crispEdges">
      <g className="cp-cat__tail">{runRects(layers.tail)}</g>
      {runRects(layers.body)}
      <g className="cp-cat__ear">{runRects(layers.ear)}</g>
      <g className={p.blink ? 'cp-cat__eyes' : undefined}>
        {eyeRects(pet, ink, INK)}
        {eyeRects(pet, glint, GLINT)}
      </g>
      {p.blink && <g className="cp-cat__lids">{eyeRects(pet, [SHUT, SHUT], INK)}</g>}
      {(mouth === 'open' ? pet.open : pet.smile).map(([x, y, w, h, key], i) =>
        cellRect(`m${i}`, x, y, w, h, layers.palette[key]),
      )}
      {item && (
        <>
          <rect
            x={ix * CELL} y={iy * CELL} width={3.5 * CELL} height={3.75 * CELL}
            fill="#ffffff" stroke={INK} strokeWidth="2"
          />
          {cellRect('l1', ix + 0.75, iy + 1, 2, 0.4, PAPER_LINE)}
          {cellRect('l2', ix + 0.75, iy + 1.9, 1.5, 0.4, PAPER_LINE)}
          {cellRect('l3', ix + 0.75, iy + 2.8, 1, 0.4, PAPER_LINE)}
          {[ix - 0.6, ix + 2.85].map((hx) => (
            <rect
              key={hx}
              x={hx * CELL} y={(iy + 1.25) * CELL} width={1.25 * CELL} height={1.25 * CELL}
              fill={layers.palette[pet.hand]} stroke={INK} strokeWidth="1.5"
            />
          ))}
        </>
      )}
    </g>
  );
}
