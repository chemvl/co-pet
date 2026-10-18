// React port of docs/design/canvas/Cat.dc.html (viewBox 0 0 160 160, same
// paths). Feet sit on the bottom edge; the top 22% is the FX zone.
import { useSyncExternalStore, type CSSProperties, type ReactElement } from 'react';
import { Icon, type IconName } from '../ds';
import type { BuiltinCategoryId, Edge } from '../shared/types';
import type { CatEyes, CatMotion, CatParts } from './pose';
import { resolveSkin, SKINS, skinsOf, type SkinId, type Species } from './catalog';
import { HeldNote, PixelPet } from './PixelPets';
import { COLS, EAR, EYES, OUTLINE, PIXEL_MAP, ROWS, TAIL, type CellBox } from './pixelCat';
import './cat.css';

/** Every species is skinnable (catalog.ts). */
export type CatFx =
  | null
  | { kind: 'category'; icon: IconName; color: BuiltinCategoryId }
  | { kind: 'question' }
  | { kind: 'agent' };

const MOUTH = '#8c3a2b';
const ZZZ = '#6b5d52';

export interface CatArtProps {
  look: Species;
  skin: SkinId;
  parts: CatParts;
  motion: CatMotion | null;
  /** one-shot length or loop period of `motion` */
  motionMs?: number;
  /** change it to restart the same motion (e.g. the machine's `seq`) */
  motionKey?: number;
  /** repeat `motion` every `motionMs` */
  motionLoop?: boolean;
  /** the cat faces right; the left edge mirrors it */
  mirrored: boolean;
  reducedMotion: boolean;
  /** which edge the pet docks on; sets the direction of `lean`, `offer` and `tilt` */
  edge?: Edge;
  /** above-head icon: category disc, question mark or the agent icon */
  fx?: CatFx;
  /** the pet is hidden: animations pause */
  hidden?: boolean;
  /** a meeting is recording: headphones and a rec dot (grey while paused) */
  recording?: { paused: boolean };
  /** accessible name; the art is decorative without it */
  label?: string;
  className?: string;
}

export function CatArt(props: CatArtProps): ReactElement {
  const { look, skin, motion, mirrored, reducedMotion, fx = null, label } = props;
  // `shut` closes the eyes by drawing them closed, so no animation runs; reduced motion skips it like `blink`.
  const parts: CatParts = motion === 'shut' && !reducedMotion ? { ...props.parts, eyes: 'closed' } : props.parts;
  const edge = props.edge ?? (mirrored ? 'left' : 'right');
  // Dots and zZ sit on the side away from the screen edge so they show while peeking.
  const fxLeft = edge === 'right';
  const visible = useSyncExternalStore(subscribeVisibility, () => document.visibilityState === 'visible');
  const paused = props.hidden || !visible;
  const rootClass = ['cp-cat', reducedMotion && 'is-still', paused && 'is-paused', props.className]
    .filter(Boolean)
    .join(' ');
  const style = {
    '--cat-dur': `${props.motionMs ?? 0}ms`,
    // +1 when "away from the edge" points right on screen (left edge), −1 otherwise.
    '--cat-dir': edge === 'left' ? 1 : -1,
  } as CSSProperties;
  const lowered = parts.zzz;
  const blink = motion === 'blink' && !reducedMotion;

  return (
    <div className={rootClass} style={style} data-edge={edge}>
      <div
        className="cp-cat__body"
        data-motion={motion ?? undefined}
        data-loop={props.motionLoop || undefined}
        key={props.motionKey}
      >
        <svg
          className="cp-cat__svg"
          viewBox="0 0 160 160"
          role={label ? 'img' : undefined}
          aria-label={label}
          aria-hidden={label ? undefined : true}
          focusable="false"
        >
          <g transform={mirrored ? 'translate(160 0) scale(-1 1)' : undefined}>
            {look === 'pixel' ? (
              <PixelCat skin={resolveSkin('pixel', skin)} parts={parts} lowered={lowered} blink={blink} />
            ) : (
              <PixelPet species={look} skin={resolveSkin(look, skin)} parts={parts} lowered={lowered} blink={blink} />
            )}
          </g>
          {parts.dots && <ThoughtDots left={fxLeft} />}
          {parts.zzz && <Zzz left={fxLeft} />}
        </svg>
      </div>
      {motion === 'flash' && !reducedMotion && <div className="cp-cat__flash" key={props.motionKey} />}
      {fx && (
        <span className="cp-cat__fx" key={`${fx.kind}-${props.motionKey ?? 0}`} aria-hidden="true">
          {fx.kind === 'category' ? (
            <span style={{ color: `var(--cat-${fx.color})`, display: 'flex' }}>
              <Icon name={fx.icon} size={16} />
            </span>
          ) : fx.kind === 'agent' ? (
            <span className="cp-cat__fx-agent">
              <Icon name="agent" size={16} />
            </span>
          ) : (
            <span className="cp-cat__fx-q">?</span>
          )}
        </span>
      )}
      {props.recording && (
        <span
          className={`cp-cat__rec cp-cat__rec--${fxLeft ? 'left' : 'right'}${props.recording.paused ? ' is-rec-paused' : ''}`}
          aria-hidden
        >
          <Icon name="headphones" size={16} />
          <span className="cp-cat__rec-dot" />
        </span>
      )}
    </div>
  );
}

function subscribeVisibility(onChange: () => void): () => void {
  document.addEventListener('visibilitychange', onChange);
  return () => document.removeEventListener('visibilitychange', onChange);
}

// Kitty: 7-unit cells, centred across the box with the feet on the bottom edge.
const CELL = 7;
const KITTY_X = (160 - COLS * CELL) / 2;
const KITTY_Y = 160 - ROWS * CELL;

type Rect = readonly [x: number, y: number, w: number, h: number, fill: string];

/** The map's cells inside (`inside`) or outside `boxes`; same-colour cells on a row merge into one rect. */
function runs(fills: Readonly<Record<string, string>>, boxes: readonly CellBox[], inside: boolean): Rect[] {
  const out: Rect[] = [];
  PIXEL_MAP.forEach((row, y) => {
    let start = 0;
    let fill: string | undefined;
    for (let x = 0; x <= row.length; x++) {
      const keep = x < row.length && boxes.some((b) => x >= b.x && x < b.x + b.w && y >= b.y && y < b.y + b.h) === inside;
      const f = keep ? fills[row[x]] : undefined;
      if (f === fill) continue;
      if (fill) out.push([start, y, x - start, 1, fill]);
      start = x;
      fill = f;
    }
  });
  return out;
}

const KITTY: Partial<Record<SkinId, { body: Rect[]; tail: Rect[]; ear: Rect[] }>> = Object.fromEntries(
  skinsOf('pixel').map((k) => {
    const fills = { ...k.palette, o: OUTLINE };
    return [k.id, { body: runs(fills, [TAIL, EAR], false), tail: runs(fills, [TAIL], true), ear: runs(fills, [EAR], true) }];
  }),
);

/** Eye shapes in cells, relative to the eye's top cell: the iris in the skin's `e`, the rest in ink. */
type Shape = readonly (readonly [number, number, number, number])[];
const OPEN = { iris: [[0, 0, 1, 1]], ink: [[0, 1, 1, 1]] } as const;
const SHUT: Shape = [[-0.5, 1.25, 2, 0.5]];
const KITTY_EYES: Readonly<Record<CatEyes, { iris: Shape; ink: Shape }>> = {
  open: OPEN,
  wide: { iris: [[0, -1, 1, 2]], ink: [[0, 1, 1, 1]] },
  up: { iris: [[0, -1, 1, 1]], ink: [[0, 0, 1, 1]] },
  closed: { iris: [], ink: SHUT },
  happy: { iris: [], ink: [[-0.5, 1.25, 0.5, 0.5], [0, 0.75, 1, 0.5], [1, 1.25, 0.5, 0.5]] },
  // Handled per eye below.
  wink: OPEN,
};

function rects(list: readonly Rect[]) {
  return list.map(([x, y, w, h, fill], i) => (
    <rect key={i} x={x * CELL} y={y * CELL} width={w * CELL} height={h * CELL} fill={fill} />
  ));
}

/** `shape` placed at an eye's top cell. */
function eyeRects(shape: Shape, [ex, ey]: readonly [number, number], fill: string): Rect[] {
  return shape.map(([x, y, w, h]) => [ex + x, ey + y, w, h, fill]);
}

function PixelCat(p: { skin: SkinId; parts: CatParts; lowered: boolean; blink: boolean }) {
  const layers = KITTY[p.skin]!;
  const { e: iris, b: body } = SKINS[p.skin].palette;
  const eyes = EYES.flatMap((at, i) => {
    const shape = p.parts.eyes === 'wink' && i === 1 ? KITTY_EYES.closed : KITTY_EYES[p.parts.eyes];
    return [...eyeRects(shape.iris, at, iris), ...eyeRects(shape.ink, at, OUTLINE)];
  });
  return (
    <g transform={`translate(${KITTY_X} ${KITTY_Y + (p.lowered ? 4 : 0)})`} shapeRendering="crispEdges">
      <g className="cp-cat__tail">{rects(layers.tail)}</g>
      {rects(layers.body)}
      <g className="cp-cat__ear">{rects(layers.ear)}</g>
      <g className={p.blink ? 'cp-cat__eyes' : undefined}>{rects(eyes)}</g>
      {p.blink && <g className="cp-cat__lids">{rects(EYES.flatMap((at) => eyeRects(SHUT, at, OUTLINE)))}</g>}
      {p.parts.mouth === 'open' && rects([[7.5, 7, 2, 1, MOUTH]])}
      {p.parts.item && <HeldNote cell={CELL} x={7} y={10.5} hand={body} />}
    </g>
  );
}

/** Thought-cloud trail at the top corner away from the screen edge; drawn unmirrored. */
function ThoughtDots({ left }: { left: boolean }) {
  const x = (v: number) => (left ? 160 - v : v);
  return (
    <g className="cp-cat__dots" fill="#ffffff" stroke={OUTLINE} strokeWidth="2">
      <circle cx={x(120)} cy="34" r="4" />
      <circle cx={x(132)} cy="21" r="6" />
      <circle cx={x(146)} cy="9" r="8" />
    </g>
  );
}

function Zzz({ left }: { left: boolean }) {
  // On the left, anchor at the end so the letters keep reading left to right.
  const anchor = left ? 'end' : 'start';
  return (
    <g fontFamily="'Baloo 2', sans-serif" fontWeight="700" fill={ZZZ} textAnchor={anchor}>
      <text x={left ? 48 : 112} y="36" fontSize="16">z</text>
      <text x={left ? 36 : 124} y="24" fontSize="22">Z</text>
    </g>
  );
}
