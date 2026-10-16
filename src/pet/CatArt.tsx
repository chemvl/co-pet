// React port of docs/design/canvas/Cat.dc.html (viewBox 0 0 160 160, same
// paths). Feet sit on the bottom edge; the top 22% is the FX zone.
import { useSyncExternalStore, type CSSProperties, type ReactElement } from 'react';
import { Icon } from '../ds';
import type { CategoryId, Edge } from '../shared/types';
import type { CatMotion, CatParts } from './pose';
import './cat.css';

export type CatLook = 'soft' | 'sticker' | 'pixel';
export type CatSkin = 'peach' | 'tabby' | 'midnight';
export type CatFx = null | { kind: 'category'; category: CategoryId } | { kind: 'question' };

interface SkinColors { body: string; belly: string; ear: string; blush: string; eye: string; glint: string }

export const SKINS: Readonly<Record<CatSkin, SkinColors>> = {
  peach: { body: '#f4a27c', belly: '#fde8dc', ear: '#f9c9b0', blush: '#ec8a66', eye: '#2a211c', glint: '#ffffff' },
  tabby: { body: '#9a8f86', belly: '#efe9e2', ear: '#c9bfb6', blush: '#d98f7a', eye: '#2a211c', glint: '#ffffff' },
  midnight: { body: '#3b3330', belly: '#6b5d52', ear: '#5a4d47', blush: '#c97a66', eye: '#f3ece3', glint: '#3b3330' },
};

const OUTLINE = '#2a211c';
const NOSE = '#d9775a';
const PAPER_LINE = '#8f8170';
const ZZZ = '#6b5d52';

export interface CatArtProps {
  look: CatLook;
  skin: CatSkin;
  parts: CatParts;
  motion: CatMotion | null;
  /** one-shot length or loop period of `motion` */
  motionMs?: number;
  /** change it to restart the same motion (e.g. the machine's `seq`) */
  motionKey?: number;
  /** the cat faces right; the left edge mirrors it */
  mirrored: boolean;
  reducedMotion: boolean;
  /** which edge the pet docks on; sets the direction of `lean`, `offer` and `tilt` */
  edge?: Edge;
  /** above-head icon: category disc or question mark */
  fx?: CatFx;
  /** the pet is hidden: animations pause */
  hidden?: boolean;
  /** accessible name; the art is decorative without it */
  label?: string;
  className?: string;
}

export function CatArt(props: CatArtProps): ReactElement {
  const { look, skin, parts, motion, mirrored, reducedMotion, fx = null, label } = props;
  const edge = props.edge ?? (mirrored ? 'left' : 'right');
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
      <div className="cp-cat__body" data-motion={motion ?? undefined} key={props.motionKey}>
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
              <PixelCat colors={SKINS[skin]} parts={parts} lowered={lowered} blink={blink} />
            ) : (
              <VectorCat
                sticker={look === 'sticker'}
                skin={skin}
                parts={parts}
                lowered={lowered}
                blink={blink}
              />
            )}
          </g>
          {parts.dots && <ThoughtDots mirrored={mirrored} />}
          {parts.zzz && <Zzz mirrored={mirrored} />}
        </svg>
      </div>
      {motion === 'flash' && !reducedMotion && <div className="cp-cat__flash" key={props.motionKey} />}
      {fx && (
        <span className="cp-cat__fx" key={`${fx.kind}-${props.motionKey ?? 0}`} aria-hidden="true">
          {fx.kind === 'category' ? (
            <span style={{ color: `var(--cat-${fx.category})`, display: 'flex' }}>
              <Icon name={fx.category} size={16} />
            </span>
          ) : (
            <span className="cp-cat__fx-q">?</span>
          )}
        </span>
      )}
    </div>
  );
}

function subscribeVisibility(onChange: () => void): () => void {
  document.addEventListener('visibilitychange', onChange);
  return () => document.removeEventListener('visibilitychange', onChange);
}

const TAIL = 'M96 98 C120 96 120 66 104 62';

function VectorCat(p: { sticker: boolean; skin: CatSkin; parts: CatParts; lowered: boolean; blink: boolean }) {
  const s = SKINS[p.skin];
  // The sticker canvas palette lightens belly and inner ear on the peach skin.
  const belly = p.sticker && p.skin === 'peach' ? '#fff7ee' : s.belly;
  const ear = p.sticker && p.skin === 'peach' ? '#fde8dc' : s.ear;
  const line = p.sticker ? OUTLINE : 'none';
  const sw = p.sticker ? 3.5 : 0;
  const eyeR = p.sticker ? 5.5 : 4.5;
  const { eyes, mouth } = p.parts;
  const openEyes = (
    <g className={p.blink ? 'cp-cat__eyes' : undefined}>
      <circle cx="47" cy="50" r={eyeR} fill={s.eye} />
      <circle cx="73" cy="50" r={eyeR} fill={s.eye} />
      <circle cx="48.6" cy="48.4" r="1.6" fill={s.glint} />
      <circle cx="74.6" cy="48.4" r="1.6" fill={s.glint} />
    </g>
  );
  const arc = (d: string, className?: string) => (
    <path className={className} d={d} fill="none" stroke={s.eye} strokeWidth="3" strokeLinecap="round" />
  );

  return (
    <g transform={p.lowered ? 'translate(20 44)' : 'translate(20 40)'}>
      {p.sticker && (
        <g fill="#ffffff" stroke="#ffffff" strokeWidth="14" strokeLinejoin="round" strokeLinecap="round">
          <path d={TAIL} fill="none" />
          <polygon points="30,32 28,4 54,20" />
          <polygon points="90,32 92,4 66,20" />
          <ellipse cx="60" cy="88" rx="38" ry="29" />
          <circle cx="60" cy="50" r="34" />
          <ellipse cx="44" cy="114" rx="10" ry="6" />
          <ellipse cx="76" cy="114" rx="10" ry="6" />
        </g>
      )}
      <g className="cp-cat__tail">
        {p.sticker && <path d={TAIL} fill="none" stroke={OUTLINE} strokeWidth="16" strokeLinecap="round" />}
        <path d={TAIL} fill="none" stroke={s.body} strokeWidth="9" strokeLinecap="round" />
      </g>
      <polygon points="30,32 28,4 54,20" fill={s.body} stroke={line} strokeWidth={sw} strokeLinejoin="round" />
      <polygon points="34,25 33,11 46,19" fill={ear} />
      <g className="cp-cat__ear">
        <polygon points="90,32 92,4 66,20" fill={s.body} stroke={line} strokeWidth={sw} strokeLinejoin="round" />
        <polygon points="86,25 87,11 74,19" fill={ear} />
      </g>
      <ellipse cx="60" cy="88" rx="38" ry="29" fill={s.body} stroke={line} strokeWidth={sw} />
      <ellipse cx="60" cy="94" rx="21" ry="17" fill={belly} />
      <ellipse cx="44" cy="114" rx="10" ry="6" fill={s.body} stroke={line} strokeWidth={sw} />
      <ellipse cx="76" cy="114" rx="10" ry="6" fill={s.body} stroke={line} strokeWidth={sw} />
      <circle cx="60" cy="50" r="34" fill={s.body} stroke={line} strokeWidth={sw} />
      {p.sticker && (
        <>
          <path d="M34 74 Q60 88 86 74" fill="none" stroke="#1d6b57" strokeWidth="7" strokeLinecap="round" />
          <circle cx="60" cy="86" r="5" fill="#ffffff" stroke={OUTLINE} strokeWidth="2" />
        </>
      )}
      <circle cx="38" cy="60" r="5" fill={s.blush} opacity="0.55" />
      <circle cx="82" cy="60" r="5" fill={s.blush} opacity="0.55" />

      {eyes === 'open' && openEyes}
      {eyes === 'open' && p.blink && arc('M41 51 Q47 56 53 51 M67 51 Q73 56 79 51', 'cp-cat__lids')}
      {eyes === 'wide' && (
        <>
          <circle cx="47" cy="49" r="6.5" fill={s.eye} />
          <circle cx="73" cy="49" r="6.5" fill={s.eye} />
          <circle cx="49" cy="47" r="2.2" fill={s.glint} />
          <circle cx="75" cy="47" r="2.2" fill={s.glint} />
        </>
      )}
      {eyes === 'up' && (
        <>
          <circle cx="49" cy="45" r={eyeR} fill={s.eye} />
          <circle cx="75" cy="45" r={eyeR} fill={s.eye} />
        </>
      )}
      {eyes === 'closed' && arc('M41 51 Q47 56 53 51 M67 51 Q73 56 79 51')}
      {eyes === 'happy' && arc('M41 52 Q47 45 53 52 M67 52 Q73 45 79 52')}
      {eyes === 'wink' && (
        <>
          <circle cx="47" cy="50" r={eyeR} fill={s.eye} />
          <circle cx="48.6" cy="48.4" r="1.6" fill={s.glint} />
          {arc('M67 51 Q73 56 79 51')}
        </>
      )}

      <polygon points="56.5,58 63.5,58 60,62" fill={NOSE} />
      {mouth === 'open' ? (
        <ellipse cx="60" cy="67" rx="5" ry="4.5" fill={s.eye} />
      ) : (
        <path d="M60 62 Q57 66 53 64 M60 62 Q63 66 67 64" fill="none" stroke={s.eye} strokeWidth="2" strokeLinecap="round" />
      )}

      {p.parts.item && (
        <>
          <rect x="47" y="72" width="26" height="28" rx="3" fill="#ffffff" stroke={OUTLINE} strokeWidth="2.5" />
          <path d="M52 81 H68 M52 87 H66 M52 93 H62" stroke={PAPER_LINE} strokeWidth="2" strokeLinecap="round" />
          <ellipse cx="45" cy="84" rx="8" ry="7" fill={s.body} stroke={line} strokeWidth={sw} />
          <ellipse cx="75" cy="84" rx="8" ry="7" fill={s.body} stroke={line} strokeWidth={sw} />
        </>
      )}
    </g>
  );
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

function PixelCat(p: { colors: SkinColors; parts: CatParts; lowered: boolean; blink: boolean }) {
  const ox = 20;
  const oy = p.lowered ? 44 : 40;
  const fills: Record<string, string> = { o: OUTLINE, b: p.colors.body, l: p.colors.belly, p: NOSE };
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
  const ink = p.colors.eye;
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

/** Thought-cloud trail, top right (top left when mirrored); drawn unmirrored so it reads the same. */
function ThoughtDots({ mirrored }: { mirrored: boolean }) {
  const x = (v: number) => (mirrored ? 160 - v : v);
  return (
    <g className="cp-cat__dots" fill="#ffffff" stroke={OUTLINE} strokeWidth="2">
      <circle cx={x(120)} cy="34" r="4" />
      <circle cx={x(132)} cy="21" r="6" />
      <circle cx={x(146)} cy="9" r="8" />
    </g>
  );
}

function Zzz({ mirrored }: { mirrored: boolean }) {
  // Mirrored: anchor at the end so the letters keep reading left to right.
  const anchor = mirrored ? 'end' : 'start';
  return (
    <g fontFamily="'Baloo 2', sans-serif" fontWeight="700" fill={ZZZ} textAnchor={anchor}>
      <text x={mirrored ? 48 : 112} y="36" fontSize="16">z</text>
      <text x={mirrored ? 36 : 124} y="24" fontSize="22">Z</text>
    </g>
  );
}
