// Line-for-line TSX port of docs/design/design-system/components/bundle.js.
// DOM, class names and defaults are identical; the only additions are the
// optional PetDock/TrayCard props documented in ./types.ts, plus ref forwarding
// on SearchField (Cmd+K focus) and Bubble (bubble arc focus). SearchFieldProps
// omits the HTML `size` attribute so `size` is the 'md' | 'lg' variant.
import { forwardRef, type CSSProperties, type ReactElement } from 'react';
import { ICONS } from './icons';
import type {
  BubbleProps, ButtonProps, CategoryChipProps, CategoryId, IconName, IconProps, ItemKind, KbdProps,
  PetBadgeProps, PetDockProps, RecordingHUDProps, SearchFieldProps, SegmentedControlProps, Status,
  StatusPillProps, SwitchProps, TaskRowProps, TranscriptSegmentProps, TrayCardProps, TrayProps,
  WhatsNextCardProps,
} from './types';

function cx(...parts: unknown[]): string {
  const out: string[] = [];
  for (const p of parts) if (p) out.push(p as string);
  return out.join(' ');
}

export const CATEGORIES: Record<CategoryId, { icon: IconName; label: string; vi: string }> = {
  todo: { icon: 'todo', label: 'To-do', vi: 'Việc cần làm' },
  read: { icon: 'read', label: 'Read later', vi: 'Đọc sau' },
  reference: { icon: 'reference', label: 'Reference', vi: 'Tài liệu' },
  code: { icon: 'code', label: 'Code snippet', vi: 'Đoạn code' },
  design: { icon: 'design', label: 'Design', vi: 'Thiết kế' },
  idea: { icon: 'idea', label: 'Idea / note', vi: 'Ý tưởng / ghi chú' },
  contact: { icon: 'contact', label: 'Contact', vi: 'Liên hệ' },
  meeting: { icon: 'meeting', label: 'Meeting', vi: 'Cuộc họp' },
  unsorted: { icon: 'unsorted', label: 'Unsorted', vi: 'Chưa phân loại' },
};
export const KIND_ICON: Record<ItemKind, IconName> = {
  file: 'file', image: 'image', url: 'link', text: 'text', richtext: 'text', code: 'code', screenshot: 'screenshot', audio: 'mic', meeting: 'meeting',
};

/* ---------- Icon ---------- */
export function Icon(p: IconProps): ReactElement {
  const size = p.size || 20;
  return (
    <svg
      className={cx('cp-icon', p.className)}
      width={size} height={size} viewBox="0 0 24 24" fill="none"
      stroke="currentColor" strokeWidth={p.strokeWidth || 1.75}
      strokeLinecap="round" strokeLinejoin="round"
      role={p.label ? 'img' : undefined} aria-label={p.label} aria-hidden={p.label ? undefined : true}
      focusable="false"
      dangerouslySetInnerHTML={{ __html: ICONS[p.name] || ICONS.unsorted }}
    />
  );
}

/* ---------- Button ---------- */
export function Button(p: ButtonProps): ReactElement {
  const { variant = 'secondary', size = 'md', icon, iconEnd, children, className, type, ...rest } = p;
  const iconOnly = !children && icon;
  const is = size === 'sm' ? 16 : 18;
  return (
    <button type={type || 'button'} {...rest}
      className={cx('cp-btn', 'cp-btn--' + variant, 'cp-btn--' + size, iconOnly && 'cp-btn--icon', className)}>
      {icon ? <Icon name={icon} size={is} /> : null}
      {children ? <span className="cp-btn__label">{children}</span> : null}
      {iconEnd ? <Icon name={iconEnd} size={is} /> : null}
    </button>
  );
}

/* ---------- Kbd ---------- */
export function Kbd(p: KbdProps): ReactElement {
  const keys = p.keys || [];
  return (
    <span className={cx('cp-kbd', p.className)} aria-label={keys.join(' + ')}>
      {keys.map((k, i) => <kbd key={i}>{k}</kbd>)}
    </span>
  );
}

/* ---------- CategoryChip ---------- */
export function CategoryChip(p: CategoryChipProps): ReactElement {
  const known = CATEGORIES[p.category];
  const c = known || CATEGORIES.unsorted;
  const label = p.label || (p.lang === 'vi' ? c.vi : c.label);
  return (
    <span className={cx('cp-chip', 'cp-chip--' + (known ? p.category : 'unsorted'), p.size === 'sm' && 'cp-chip--sm', p.className)}>
      <Icon name={p.icon || c.icon} size={p.size === 'sm' ? 14 : 16} />
      {p.iconOnly ? null : <span>{label}</span>}
      {p.count != null ? <span className="cp-chip__count">{p.count}</span> : null}
    </span>
  );
}

/* ---------- StatusPill ---------- */
export const STATUS: Record<Status, { icon: IconName; tone: string; label: string }> = {
  synced: { icon: 'check', tone: 'success', label: 'Synced' },
  exported: { icon: 'check', tone: 'success', label: 'Exported' },
  pending: { icon: 'clock', tone: 'neutral', label: 'Not exported' },
  exporting: { icon: 'export', tone: 'info', label: 'Exporting…' },
  failed: { icon: 'alert', tone: 'danger', label: 'Export failed' },
  due: { icon: 'clock', tone: 'warning', label: 'Due soon' },
  overdue: { icon: 'alert', tone: 'danger', label: 'Overdue' },
  local: { icon: 'lock', tone: 'neutral', label: 'Local only' },
  sensitive: { icon: 'lock', tone: 'warning', label: 'Sensitive' },
  agent: { icon: 'agent', tone: 'info', label: 'Added by agent' },
};
export function StatusPill(p: StatusPillProps): ReactElement {
  const s = STATUS[p.status] || STATUS.pending;
  return (
    <span className={cx('cp-pill', 'cp-pill--' + s.tone, p.className)}>
      <Icon name={p.icon || s.icon} size={14} /><span>{p.label || s.label}</span>
    </span>
  );
}

/* ---------- PetBadge ---------- */
export function PetBadge(p: PetBadgeProps): ReactElement | null {
  if (p.dot) return <span className={cx('cp-badge', 'cp-badge--dot', p.className)} role="status" aria-label={p.label || 'A task is due soon'} />;
  if (!p.count) return null;
  const txt = p.count > 99 ? '99+' : String(p.count);
  return <span className={cx('cp-badge', p.className)} role="status" aria-label={p.label || (p.count + ' unsorted items')}>{txt}</span>;
}

/* ---------- Bubble ---------- */
export const Bubble = forwardRef<HTMLButtonElement, BubbleProps>(function Bubble(p, ref) {
  const { icon, label, active, showLabel, className, labelSide, style, ...rest } = p;
  return (
    <button ref={ref} type="button" aria-label={label} {...rest}
      className={cx('cp-bubble', active && 'is-active', className)} style={style}>
      <Icon name={icon} size={20} />
      {showLabel ? <span className={cx('cp-bubble__tip', 'cp-bubble__tip--' + (labelSide || 'top'))} aria-hidden>{label}</span> : null}
    </button>
  );
});

/* ---------- PetDock ---------- */
export const DEFAULT_BUBBLES: { icon: IconName; label: string }[] = [
  { icon: 'whats-next', label: "What's next" },
  { icon: 'screenshot', label: 'Capture screen' },
  { icon: 'clipboard', label: 'Save clipboard' },
  { icon: 'mic', label: 'Record meeting' },
  { icon: 'home', label: 'Open home' },
  { icon: 'note', label: 'Quick note' },
];
function PetSlot(p: { sleep: boolean }): ReactElement {
  // Placeholder silhouette for the Rive artboard; replace with the species canvas.
  return (
    <svg className="cp-pet__art" viewBox="0 0 80 80" aria-hidden>
      <path className="cp-pet__body" d="M14 70c-4-26 6-44 26-44s30 18 26 44z" />
      <path className="cp-pet__body" d="M20 34 18 14l16 12zM60 34l2-20-16 12z" />
      <circle className="cp-pet__eye" cx={32} cy={46} r={p.sleep ? 0 : 3} />
      <circle className="cp-pet__eye" cx={48} cy={46} r={p.sleep ? 0 : 3} />
      {p.sleep ? <path className="cp-pet__lid" d="M28 46h8M44 46h8" /> : null}
    </svg>
  );
}
/** `ref` is the plan's `petRef`: it points at the `.cp-pet` button. */
export const PetDock = forwardRef<HTMLButtonElement, PetDockProps>(function PetDock(p, petRef) {
  const edge = p.edge || 'right', state = p.state || 'peek';
  const size = p.size || 80;
  const bubbles = p.bubbles || DEFAULT_BUBBLES;
  const out = state !== 'peek' && state !== 'sleep';
  const showBubbles = state === 'hover';
  const arc = p.arc || 100;
  // fan the bubbles away from the edge, across a 140° arc
  const base = { right: 180, left: 0, bottom: 270 }[edge];
  const n = bubbles.length, spread = n > 1 ? 140 : 0;
  const items = bubbles.map((b, i) => {
    const a = (base - spread / 2 + (n > 1 ? (spread * i) / (n - 1) : 0)) * Math.PI / 180;
    let x = Math.cos(a) * arc, y = -Math.sin(a) * arc;
    if (edge === 'bottom') { x = Math.cos(a + Math.PI) * arc; y = Math.sin(a) * arc; }
    return (
      <Bubble key={b.icon} icon={b.icon} label={b.label} showLabel={i === (p.focusIndex == null ? -1 : p.focusIndex)}
        labelSide={edge === 'bottom' ? 'top' : edge === 'right' ? 'left' : 'right'}
        className="cp-dock__bubble" style={{ transform: 'translate(' + x.toFixed(1) + 'px,' + y.toFixed(1) + 'px)', transitionDelay: (i * 30) + 'ms' }}
        onClick={p.onBubble ? () => p.onBubble!(b.icon, i) : undefined}
        ref={p.bubbleFocusRef ? (el) => p.bubbleFocusRef!(i, el) : undefined} />
    );
  });
  return (
    <div className={cx('cp-dock', 'cp-dock--' + edge, 'is-' + state, p.className)} style={{ '--pet': size + 'px' } as CSSProperties}>
      <div className="cp-dock__anchor">
        {state === 'drag' ? <div className="cp-dock__halo" aria-hidden /> : null}
        {showBubbles ? <div className="cp-dock__bubbles" role="toolbar" aria-label={p.bubblesLabel || 'Pet actions'}>{items}</div> : null}
        <button type="button" aria-label={(p.petName || 'Pet') + ' — click for tray, double-click for Home'} {...p.petProps}
          ref={petRef} className={cx('cp-pet', out && 'is-out', p.petProps?.className)}>
          {p.art !== undefined ? p.art : <PetSlot sleep={state === 'sleep'} />}
          {p.badge ? <PetBadge {...(typeof p.badge === 'object' ? p.badge : { count: p.badge, className: 'cp-pet__badge' })} /> : null}
        </button>
      </div>
    </div>
  );
});

/* ---------- SearchField ---------- */
export const SearchField = forwardRef<HTMLInputElement, SearchFieldProps>(function SearchField(p, ref) {
  const { shortcut, className, size, ...rest } = p;
  return (
    <label className={cx('cp-search', size === 'lg' && 'cp-search--lg', className)}>
      <Icon name="search" size={18} />
      <input ref={ref} type="search" aria-label={p.placeholder || 'Search'} {...rest} />
      {shortcut ? <Kbd keys={shortcut} /> : null}
    </label>
  );
});

/* ---------- SegmentedControl ---------- */
export function SegmentedControl(p: SegmentedControlProps): ReactElement {
  const opts = p.options || [];
  return (
    <div className={cx('cp-seg', p.className)} role="tablist" aria-label={p.label}>
      {opts.map((o) => {
        const sel = o.value === p.value;
        return (
          <button key={o.value} type="button" role="tab" aria-selected={sel} className={cx('cp-seg__opt', sel && 'is-selected')}
            onClick={() => { p.onChange && p.onChange(o.value); }}>
            {o.icon ? <Icon name={o.icon} size={16} /> : null}
            <span>{o.label}</span>
            {o.count != null ? <span className="cp-seg__count">{o.count}</span> : null}
          </button>
        );
      })}
    </div>
  );
}

/* ---------- Switch ---------- */
export function Switch(p: SwitchProps): ReactElement {
  const id = p.id || ('sw-' + (p.label || '').replace(/\W+/g, '-').toLowerCase());
  return (
    <div className={cx('cp-switch', p.disabled && 'is-disabled', p.className)}>
      <div className="cp-switch__text">
        <label htmlFor={id} className="cp-switch__label">{p.label}</label>
        {p.description ? <p className="cp-switch__desc">{p.description}</p> : null}
      </div>
      <button id={id} type="button" role="switch" aria-checked={!!p.checked} disabled={p.disabled} className="cp-switch__track"
        onClick={() => { p.onChange && p.onChange(!p.checked); }}><span className="cp-switch__thumb" /></button>
    </div>
  );
}

/* ---------- TrayCard ---------- */
export function TrayCard(p: TrayCardProps): ReactElement {
  const {
    kind, title, meta, category, thumbnail, pinned, selected, sensitive, agent, dragging, lang,
    onCopy, onPaste, labels, categoryLabel, categoryIcon, className, ...rest
  } = p;
  const kindIcon = KIND_ICON[kind] || 'file';
  return (
    <article {...rest} className={cx('cp-card', selected && 'is-selected', dragging && 'is-dragging', className)} draggable={false} tabIndex={0} aria-selected={!!selected}>
      <div className={cx('cp-card__kind', thumbnail && 'has-thumb')}>
        {thumbnail ? <img src={thumbnail} alt="" /> : <Icon name={kindIcon} size={20} />}
      </div>
      <div className="cp-card__body">
        <div className="cp-card__title">
          {pinned ? <Icon name="pin" size={14} label="Pinned" className="cp-card__pin" /> : null}
          <span title={title}>{title}</span>
        </div>
        {meta ? <div className="cp-card__meta">{meta}</div> : null}
        <div className="cp-card__tags">
          {category ? <CategoryChip category={category} size="sm" lang={lang} label={categoryLabel} icon={categoryIcon} /> : null}
          {sensitive ? <StatusPill status="sensitive" label={labels?.sensitive} /> : null}
          {agent ? <StatusPill status="agent" label={labels?.agent} /> : null}
        </div>
      </div>
      <div className="cp-card__actions">
        <Button variant="quiet" size="sm" icon="copy" aria-label={labels?.copy || 'Copy'} onClick={onCopy} />
        <Button variant="quiet" size="sm" icon="paste" aria-label={labels?.paste || 'Paste into focused field'} onClick={onPaste} />
      </div>
    </article>
  );
}

/* ---------- Tray ---------- */
export function Tray(p: TrayProps): ReactElement {
  return (
    <section className={cx('cp-tray', p.className)} aria-label={p.label || 'Tray'}>
      <header className="cp-tray__head">{p.search}{p.filters}</header>
      <div className="cp-tray__list" role="list">{p.children}</div>
      {p.footer ? <footer className="cp-tray__foot">{p.footer}</footer> : null}
    </section>
  );
}

/* ---------- WhatsNextCard ---------- */
export function WhatsNextCard(p: WhatsNextCardProps): ReactElement {
  const L = Object.assign({ eyebrow: "What's next", start: 'Start', done: 'Done', snooze: 'Snooze', skip: 'Skip', firstStep: 'First step' }, p.labels || {});
  return (
    <section className={cx('cp-next', p.className)} aria-label={L.eyebrow}>
      <div className="cp-next__eyebrow"><Icon name="whats-next" size={16} /><span>{L.eyebrow}</span></div>
      <h3 className="cp-next__title">{p.title}</h3>
      <div className="cp-next__meta">
        {p.estimate ? <span><Icon name="clock" size={14} />{p.estimate}</span> : null}
        {p.due ? <span className={p.dueSoon ? 'is-due' : undefined}><Icon name="calendar" size={14} />{p.due}</span> : null}
        {p.category ? <CategoryChip category={p.category} size="sm" lang={p.lang} label={p.categoryLabel} icon={p.categoryIcon} /> : null}
      </div>
      {p.reason ? <p className="cp-next__reason">{p.reason}</p> : null}
      {p.firstStep ? <div className="cp-next__step"><span className="cp-next__step-label">{L.firstStep}</span><span>{p.firstStep}</span></div> : null}
      <div className="cp-next__actions">
        <Button variant="primary" size="sm" icon="play" onClick={p.onStart}>{L.start}</Button>
        <Button variant="secondary" size="sm" icon="check" onClick={p.onDone}>{L.done}</Button>
        <span className="cp-next__spacer" />
        <Button variant="quiet" size="sm" onClick={p.onSnooze}>{L.snooze}</Button>
        <Button variant="quiet" size="sm" onClick={p.onSkip}>{L.skip}</Button>
      </div>
    </section>
  );
}

/* ---------- RecordingHUD ---------- */
function Meter(p: { label: string; level?: number }): ReactElement {
  const bars = 8, lit = Math.round((p.level || 0) * bars);
  const cells: ReactElement[] = [];
  for (let i = 0; i < bars; i++) cells.push(<i key={i} className={cx(i < lit && 'on', i === bars - 1 && i < lit && 'peak')} />);
  return <span className="cp-hud__meter"><span className="cp-hud__meter-label">{p.label}</span><span className="cp-hud__bars" aria-hidden>{cells}</span></span>;
}
export function RecordingHUD(p: RecordingHUDProps): ReactElement {
  const L = Object.assign({ me: 'Me', system: 'System', pause: 'Pause', resume: 'Resume', stop: 'Stop', recording: 'Recording', paused: 'Paused' }, p.labels || {});
  return (
    <div className={cx('cp-hud', p.paused && 'is-paused', p.className)} role="group" aria-label={p.paused ? L.paused : L.recording}>
      <span className="cp-hud__dot" aria-hidden />
      <span className="cp-hud__time" role="timer">{p.elapsed || '00:00'}</span>
      <Meter label={L.me} level={p.paused ? 0 : p.micLevel} />
      <Meter label={L.system} level={p.paused ? 0 : p.systemLevel} />
      <Button variant="quiet" size="sm" icon="language" iconEnd="chevron-down" className="cp-hud__lang">{p.language || 'Auto'}</Button>
      <Button variant="quiet" size="sm" icon={p.paused ? 'play' : 'pause'} aria-label={p.paused ? L.resume : L.pause} onClick={p.onPause} />
      <Button variant="danger" size="sm" icon="stop" onClick={p.onStop}>{L.stop}</Button>
    </div>
  );
}

/* ---------- TaskRow ---------- */
export function TaskRow(p: TaskRowProps): ReactElement {
  const L = Object.assign({ markDone: 'Mark done', actions: 'Task actions' }, p.labels || {});
  return (
    <div className={cx('cp-task', p.done && 'is-done', p.selected && 'is-selected', p.className)} role="listitem"
      tabIndex={p.onOpen ? 0 : undefined}
      onKeyDown={p.onOpen ? (e) => { if (e.key === 'Enter' && e.target === e.currentTarget) { e.preventDefault(); p.onOpen!(); } } : undefined}>
      <button type="button" role="checkbox" aria-checked={!!p.done} aria-label={L.markDone} className="cp-task__check" onClick={p.onToggle}>
        {p.done ? <Icon name="check" size={14} strokeWidth={2.5} /> : null}
      </button>
      <div className="cp-task__body" onClick={p.onOpen}>
        <div className="cp-task__title">{p.title}</div>
        <div className="cp-task__meta">
          {p.source ? <span><Icon name={p.source === 'meeting' ? 'meeting' : 'inbox'} size={14} />{p.sourceLabel}</span> : null}
          {p.estimate ? <span><Icon name="clock" size={14} />{p.estimate}</span> : null}
          {p.due ? <span className={p.overdue ? 'is-overdue' : p.dueSoon ? 'is-due' : undefined}><Icon name={p.overdue ? 'alert' : 'calendar'} size={14} />{p.due}</span> : null}
        </div>
      </div>
      <div className="cp-task__side">
        {p.exportStatus ? (
          p.onExportClick ? (
            <button type="button" className="cp-task__export" onClick={p.onExportClick}>
              <StatusPill status={p.exportStatus} label={p.exportLabel} />
            </button>
          ) : (
            <StatusPill status={p.exportStatus} label={p.exportLabel} />
          )
        ) : null}
        <Button variant="quiet" size="sm" icon="more" aria-label={L.actions} aria-haspopup={p.onActions ? 'menu' : undefined}
          onClick={p.onActions ? (e) => p.onActions!(e.currentTarget) : undefined} />
      </div>
    </div>
  );
}

/* ---------- TranscriptSegment ---------- */
export function TranscriptSegment(p: TranscriptSegmentProps): ReactElement {
  return (
    <div className={cx('cp-seg-line', p.active && 'is-active', p.className)}>
      <button type="button" className="cp-seg-line__time" aria-label={'Play from ' + p.start}>{p.start}</button>
      <div className="cp-seg-line__body">
        <div className="cp-seg-line__head">
          <span className={cx('cp-speaker', p.isMe ? 'cp-speaker--me' : 'cp-speaker--n' + ((p.speakerIndex || 1) % 4))}>{p.speaker}</span>
          {p.language ? <span className="cp-seg-line__lang">{p.language}</span> : null}
        </div>
        <p className="cp-seg-line__text">{p.children || p.text}</p>
        {p.taskChange ? <div className="cp-seg-line__change"><Icon name="tasks" size={14} /><span>{p.taskChange}</span></div> : null}
      </div>
    </div>
  );
}
