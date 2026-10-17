// Kitty's 22×17 pixel art, shared by CatArt and scripts/render-icons.ts (pure data, no React).
// Keys: o outline, b body, s stripes, l belly, i inner ear; '.' is empty. The skin sets b, s, l
// and i (catalog.ts); plain skins give s and l the body colour. Eyes are an overlay.

export const OUTLINE = '#2a211c';

export const PIXEL_MAP: readonly string[] = [
  '...ooo.....ooo........',
  '..oibbooooobbio.......',
  '..oiibsbsbsbiio.......',
  '..obbbsbbbsbbbo.......',
  '..obbbbbbbbbbbo.......',
  '..osbbbbbbbbbso.......',
  'ooobbbbbobbbbbooo.....',
  '..osbbbbbbbbbso.......',
  'ooobbbbbbbbbbbooo.....',
  '...oobbbbbbboo....ooo.',
  '.....obbbbbo.....ossso',
  '.....ollbbsso...osbsbo',
  '.....olllbbsso.obsooo.',
  '.....ollllbbssobbo....',
  '.....ollllbbbbsbo.....',
  '.....obbobbobboo......',
  '......oooooooo........',
];

export const COLS = 22;
export const ROWS = PIXEL_MAP.length;

/** Cells drawn in the tail layer (behind the body) and the ear layer (in front), so the fidgets move them. */
export interface CellBox { x: number; y: number; w: number; h: number }
export const TAIL: CellBox = { x: 15, y: 9, w: 7, h: 5 };
export const EAR: CellBox = { x: 11, y: 0, w: 4, h: 2 };

/** Column, row of each eye's top cell, viewer's left first: the iris (skin `e`) sits on the pupil. */
export const EYES: readonly (readonly [number, number])[] = [[6, 5], [10, 5]];
