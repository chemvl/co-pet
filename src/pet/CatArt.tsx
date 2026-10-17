// React port of docs/design/canvas/Cat.dc.html (viewBox 0 0 160 160, same
// paths). Feet sit on the bottom edge; the top 22% is the FX zone.
import { useSyncExternalStore, type CSSProperties, type ReactElement } from 'react';
import { Icon, type IconName } from '../ds';
import type { BuiltinCategoryId, Edge } from '../shared/types';
import type { CatMotion, CatParts } from './pose';
import { resolveSkin, SKINS, type SkinId, type Species } from './catalog';
import { PixelPet } from './PixelPets';
import './cat.css';

/** Every species is skinnable (catalog.ts). */
export type CatFx =
  | null
  | { kind: 'category'; icon: IconName; color: BuiltinCategoryId }
  | { kind: 'question' }
  | { kind: 'agent' };

const OUTLINE = '#2a211c';
const NOSE = '#d9775a';
const PAPER_LINE = '#8f8170';
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
  const { look, skin, parts, motion, mirrored, reducedMotion, fx = null, label } = props;
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
              <PixelCat palette={SKINS[resolveSkin('pixel', skin)].palette} parts={parts} lowered={lowered} blink={blink} />
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

const PIXEL_MAP = [
  '.o........o.',
  '.oo......oo.',
  '.olo....olo.',
  '.obboooobbo.',
  'obbbbbbbbbbo',
  'obbbbbbbbbbo',
  'obbbbppbbbbo',
  '.obbllllbbo.',
  '.obbllllbbo.',
  '.obbbbbbbboo',
  '.obbo..obbob',
  '..oo....oo..',
];

interface Px { x: number; y: number; w: number; h: number }

function PixelCat(p: { palette: Readonly<Record<string, string>>; parts: CatParts; lowered: boolean; blink: boolean }) {
  const ox = 20;
  const oy = p.lowered ? 44 : 40;
  const fills: Record<string, string> = { o: OUTLINE, b: p.palette.b, l: p.palette.l, p: NOSE };
  const base: (Px & { f: string })[] = [];
  PIXEL_MAP.forEach((row, r) => {
    [...row].forEach((ch, k) => {
      const f = fills[ch];
      if (f) base.push({ x: ox + k * 10, y: oy + r * 10, w: 10, h: 10, f });
    });
  });
  const cell = (cx: number, cy: number): Px => ({ x: ox + cx * 10, y: oy + cy * 10, w: 10, h: 10 });
  const leftShut: Px = { x: ox + 25, y: oy + 54, w: 20, h: 4 };
  const rightShut: Px = { x: ox + 75, y: oy + 54, w: 20, h: 4 };
  // Happy eyes: a small "^" per eye.
  const caret = (x: number): Px[] => [
    { x, y: oy + 52, w: 6, h: 4 },
    { x: x + 6, y: oy + 48, w: 8, h: 4 },
    { x: x + 14, y: oy + 52, w: 6, h: 4 },
  ];
  let eyes: Px[];
  switch (p.parts.eyes) {
    case 'open': eyes = [cell(3, 5), cell(8, 5)]; break;
    case 'wide': eyes = [cell(3, 4), cell(3, 5), cell(8, 4), cell(8, 5)]; break;
    case 'up': eyes = [cell(3, 4), cell(8, 4)]; break;
    case 'closed': eyes = [leftShut, rightShut]; break;
    case 'happy': eyes = [...caret(ox + 25), ...caret(ox + 75)]; break;
    case 'wink': eyes = [cell(3, 5), rightShut]; break;
  }
  const mouth = p.parts.mouth === 'open' ? [cell(5, 7), cell(6, 7)] : [];
  const ink = p.palette.e;
  const rects = (list: Px[], fill: string) =>
    list.map((r, i) => <rect key={i} x={r.x} y={r.y} width={r.w} height={r.h} fill={fill} />);

  return (
    <g shapeRendering="crispEdges">
      {base.map((r, i) => (
        <rect key={i} x={r.x} y={r.y} width={r.w} height={r.h} fill={r.f} />
      ))}
      <g className={p.blink ? 'cp-cat__eyes' : undefined}>{rects(eyes, ink)}</g>
      {p.blink && <g className="cp-cat__lids">{rects([leftShut, rightShut], ink)}</g>}
      {rects(mouth, ink)}
      {p.parts.item && (
        <>
          <rect x="68" y="4" width="24" height="28" fill="#ffffff" stroke={OUTLINE} strokeWidth="3" />
          <rect x="73" y="12" width="14" height="3" fill={PAPER_LINE} />
          <rect x="73" y="19" width="10" height="3" fill={PAPER_LINE} />
        </>
      )}
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
