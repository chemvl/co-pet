// Building blocks shared by the Settings sections, onboarding and other Home screens.
import { useEffect, useId, useRef, useState, type ReactElement, type ReactNode } from 'react';
import { Button } from '../../ds';
import { resolveSkin, skinsOf, SPECIES, swatchOf, type SkinId, type Species } from '../../pet/catalog';
import { CatArt } from '../../pet/CatArt';
import { poseFor } from '../../pet/pose';
import { errorMessage } from '../../shared/errors';
import { useT } from '../../shared/i18n';
import { useToast } from '../../shared/Toast';
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
export function StillCat({ look, skin, label }: { look: Species; skin: SkinId; label?: string }) {
  return <CatArt look={look} skin={skin} parts={IDLE.parts} motion={null} mirrored={false} reducedMotion label={label} />;
}

/** One tile per species, each drawn in its resolved skin. */
export function SpeciesTiles(props: { value: Species; skin: SkinId; onChange: (s: Species) => void; labelledBy?: string }) {
  const t = useT();
  return (
    <div className="st-tiles" role="group" aria-labelledby={props.labelledBy}>
      {SPECIES.map((s) => (
        <button
          key={s.id}
          type="button"
          className="st-tile"
          aria-pressed={props.value === s.id}
          onClick={() => props.onChange(s.id)}
        >
          <span className="st-tile__art"><StillCat look={s.id} skin={resolveSkin(s.id, props.skin)} /></span>
          {t(s.label)}
        </button>
      ))}
    </div>
  );
}

/** The three skins of `species`; `value` falls back to the species' default for the pressed state. */
export function SkinSwatches(props: { species: Species; value: SkinId; onChange: (skin: SkinId) => void; labelledBy?: string }) {
  const t = useT();
  const current = resolveSkin(props.species, props.value);
  return (
    <div className="st-swatches" role="group" aria-labelledby={props.labelledBy}>
      {skinsOf(props.species).map((entry) => (
        <button
          key={entry.id}
          type="button"
          className="st-swatch"
          aria-pressed={current === entry.id}
          onClick={() => props.onChange(entry.id)}
        >
          <span className="st-swatch__dot" style={{ background: swatchOf(entry.id) }} aria-hidden="true" />
          {t(entry.label)}
        </button>
      ))}
    </div>
  );
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
    <div className="hm-scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="st-dialog" role={p.role ?? 'dialog'} aria-modal="true" aria-labelledby={titleId}>
        <h2 className="st-dialog__title" id={titleId}>{p.title}</h2>
        {p.children}
        <div className="st-dialog__actions">{p.actions}</div>
      </div>
    </div>
  );
}

const COPIED_MS = 1500;

/** A button label that reads "Copied" for a moment after `copy` succeeds. `write` defaults to the
 *  plain clipboard; secrets pass `copyConcealedText`. */
export function useCopied(
  write: (text: string) => Promise<void> = (text) => navigator.clipboard.writeText(text),
): [boolean, (text: string) => void] {
  const t = useT();
  const { show } = useToast();
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const id = window.setTimeout(() => setCopied(false), COPIED_MS);
    return () => window.clearTimeout(id);
  }, [copied]);
  const copy = (text: string) => {
    write(text).then(
      () => setCopied(true),
      (e) => show({ message: errorMessage(t, e) }),
    );
  };
  return [copied, copy];
}

/** Folds the two Vietnamese tone placements ("xoá" / "xóa") and case before comparing. */
function normalizePhrase(s: string): string {
  return s.normalize('NFC').trim().toLowerCase().replace(/\s+/g, ' ').replace(/oá/g, 'óa');
}

/**
 * An irreversible action confirmed by typing `phrase` (Delete everything, Delete cloud data).
 * `onConfirm` gets the phrase for the command; a rejection shows `failed(e)` and keeps it open.
 */
export function TypedConfirmDialog(p: {
  title: string;
  body: string;
  phrase: string;
  action: string;
  variant: 'danger' | 'secondary';
  onConfirm: (phrase: string) => Promise<void>;
  failed: (e: unknown) => string;
  onClose: () => void;
}): ReactElement {
  const t = useT();
  const titleId = useId();
  const descId = useId();
  const inputId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const matches = normalizePhrase(text) === normalizePhrase(p.phrase);
  const { onClose } = p;

  useEffect(() => {
    inputRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const confirm = async () => {
    if (!matches || busy) return;
    setBusy(true);
    setError(null);
    try {
      await p.onConfirm(p.phrase);
      onClose();
    } catch (e) {
      setError(p.failed(e));
      setBusy(false);
    }
  };

  return (
    <div className="hm-scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="st-dialog" role="alertdialog" aria-modal="true" aria-labelledby={titleId} aria-describedby={descId}>
        <h2 className="st-dialog__title" id={titleId}>{p.title}</h2>
        <p className="st-body" id={descId}>{p.body}</p>
        <label className="st-row__label" htmlFor={inputId}>
          {t('settings.data.deletePrompt', { phrase: p.phrase })}
        </label>
        <input
          id={inputId}
          ref={inputRef}
          className="st-input"
          value={text}
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void confirm();
          }}
        />
        {error ? <p className="st-error" role="alert">{error}</p> : null}
        <div className="st-dialog__actions">
          <Button variant="quiet" onClick={onClose}>{t('common.cancel')}</Button>
          <Button variant={p.variant} icon="trash" disabled={!matches || busy} onClick={() => void confirm()}>
            {p.action}
          </Button>
        </div>
      </div>
    </div>
  );
}
