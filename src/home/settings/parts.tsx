// Building blocks shared by the Settings sections, onboarding and other Home screens.
import { useEffect, useId, type ReactElement, type ReactNode } from 'react';
import { CatArt, LOOKS, SKINS, type CatLook, type CatSkin } from '../../pet/CatArt';
import { poseFor } from '../../pet/pose';
import { useT, type I18nKey } from '../../shared/i18n';
import './settings.css';

export function Group({ title, desc, children }: { title?: string; desc?: string; children: ReactNode }) {
  const id = useId();
  return (
    <section className="st-group" aria-labelledby={title ? id : undefined}>
      {title ? (
        <div>
          <h2 className="st-h2" id={id}>{title}</h2>
          {desc ? <p className="st-desc">{desc}</p> : null}
        </div>
      ) : null}
      {children}
    </section>
  );
}

/** Label + one-line description on the left, the control on the right (wraps when narrow). */
export function SettingRow(props: { label: string; desc?: string; htmlFor?: string; children: ReactNode }) {
  return (
    <div className="st-row">
      <div className="st-row__text">
        {props.htmlFor ? (
          <label className="st-row__label" htmlFor={props.htmlFor}>{props.label}</label>
        ) : (
          <span className="st-row__label">{props.label}</span>
        )}
        {props.desc ? <p className="st-desc">{props.desc}</p> : null}
      </div>
      <div className="st-row__control">{props.children}</div>
    </div>
  );
}

const TIME = /^\d{2}:\d{2}$/;

/** From/To `HH:MM` inputs; `onChange` fires only when both values are valid. */
export function TimeRange(props: { start: string; end: string; onChange: (range: { start: string; end: string }) => void }) {
  const t = useT();
  const update = (patch: { start?: string; end?: string }) => {
    const next = { start: props.start, end: props.end, ...patch };
    if (TIME.test(next.start) && TIME.test(next.end)) props.onChange(next);
  };
  return (
    <div className="st-row" style={{ justifyContent: 'flex-start' }}>
      <label className="st-field">
        {t('settings.notifications.from')}
        <input type="time" className="st-input st-input--time" value={props.start} onChange={(e) => update({ start: e.target.value })} />
      </label>
      <label className="st-field">
        {t('settings.notifications.to')}
        <input type="time" className="st-input st-input--time" value={props.end} onChange={(e) => update({ end: e.target.value })} />
      </label>
    </div>
  );
}

/** `value` 0..1; null shows an indeterminate strip. */
export function ProgressBar({ value, label }: { value: number | null; label: string }) {
  const pct = value == null ? null : Math.max(0, Math.min(1, value)) * 100;
  return (
    <div
      className={pct == null ? 'st-progress is-indeterminate' : 'st-progress'}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct == null ? undefined : Math.floor(pct)}
    >
      <i style={pct == null ? undefined : { width: `${pct}%` }} />
    </div>
  );
}

const IDLE = poseFor('idle');

/** A still idle cat, for tiles and avatars. */
export function StillCat({ look, skin, label }: { look: CatLook; skin: CatSkin; label?: string }) {
  return <CatArt look={look} skin={skin} parts={IDLE.parts} motion={null} mirrored={false} reducedMotion label={label} />;
}

export const LOOK_LABEL: Record<CatLook, I18nKey> = {
  soft: 'settings.appearance.styleSoft',
  sticker: 'settings.appearance.styleSticker',
  pixel: 'settings.appearance.stylePixel',
  chicken: 'settings.appearance.styleChicken',
  dog: 'settings.appearance.styleDog',
  pig: 'settings.appearance.stylePig',
  owl: 'settings.appearance.styleOwl',
};

export function StyleTiles(props: { value: CatLook; skin: CatSkin; onChange: (look: CatLook) => void; labelledBy?: string }) {
  const t = useT();
  return (
    <div className="st-tiles" role="group" aria-labelledby={props.labelledBy}>
      {LOOKS.map((look) => (
        <button
          key={look}
          type="button"
          className="st-tile"
          aria-pressed={props.value === look}
          onClick={() => props.onChange(look)}
        >
          <span className="st-tile__art"><StillCat look={look} skin={props.skin} /></span>
          {t(LOOK_LABEL[look])}
        </button>
      ))}
    </div>
  );
}

const SKIN_ORDER: readonly CatSkin[] = ['peach', 'tabby', 'midnight'];
const SKIN_LABEL: Record<CatSkin, I18nKey> = {
  peach: 'settings.appearance.skinPeach',
  tabby: 'settings.appearance.skinTabby',
  midnight: 'settings.appearance.skinMidnight',
};

export function SkinSwatches(props: { value: CatSkin; onChange: (skin: CatSkin) => void; labelledBy?: string }) {
  const t = useT();
  return (
    <div className="st-swatches" role="group" aria-labelledby={props.labelledBy}>
      {SKIN_ORDER.map((skin) => (
        <button
          key={skin}
          type="button"
          className="st-swatch"
          aria-pressed={props.value === skin}
          onClick={() => props.onChange(skin)}
        >
          <span className="st-swatch__dot" style={{ background: SKINS[skin].body }} aria-hidden="true" />
          {t(SKIN_LABEL[skin])}
        </button>
      ))}
    </div>
  );
}

export function asLook(v: unknown): CatLook {
  return LOOKS.find((l) => l === v) ?? 'soft';
}

export function asSkin(v: unknown): CatSkin {
  return v === 'tabby' || v === 'midnight' ? v : 'peach';
}

/** A modal dialog: Escape and a click on the scrim close it. */
export function Dialog(p: {
  title: string;
  role?: 'dialog' | 'alertdialog';
  onClose: () => void;
  children: ReactNode;
  actions: ReactNode;
}): ReactElement {
  const titleId = useId();
  const { onClose } = p;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);
  return (
    <div className="st-scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="st-dialog" role={p.role ?? 'dialog'} aria-modal="true" aria-labelledby={titleId}>
        <h2 className="st-dialog__title" id={titleId}>{p.title}</h2>
        {p.children}
        <div className="st-dialog__actions">{p.actions}</div>
      </div>
    </div>
  );
}
