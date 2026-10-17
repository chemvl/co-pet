// The command palette (Palette.dc.html): a large search field over grouped results — Actions,
// Items, Tasks — with matches highlighted. ↑/↓ move, Enter runs or opens, Tab opens an item's
// or task's secondary list, Esc goes back or closes.
import { useQuery } from '@tanstack/react-query';
import { useEffect, useId, useState, type KeyboardEvent, type MouseEvent, type ReactElement, type ReactNode } from 'react';
import { Icon, KIND_ICON, Kbd, SearchField, StatusPill } from '../../ds';
import type { IconName } from '../../ds/types';
import { errorMessage } from '../../shared/errors';
import { useT } from '../../shared/i18n';
import { copyItems, exportTask, getItem, listItems } from '../../shared/ipc';
import { useIntegrations, useSetting, useSyncStatus } from '../../shared/queries';
import { displayKind } from '../../shared/itemKind';
import { useToast } from '../../shared/Toast';
import type { ItemSummary, TaskSummary } from '../../shared/types';
import { CategoryTag } from '../CategoryTag';
import { useDebounced } from '../hooks';
import { exportPill, isReady, PROVIDER_NAME, PROVIDERS } from '../integrations/model';
import { tasksQuery } from '../queries';
import { useMeetingActions } from '../meetings/recordFlow';
import { navigate } from '../router';
import { useHomeSelection } from '../selection';
import { useTaskActions } from '../tasks/useTaskActions';
import { useItemActions } from '../useItemActions';
import { matchActions, SEARCH_INBOX, type ActionMatch, type PaletteContext } from './actions';
import { matchRanges } from './match';
import { resultsSettled } from './settled';
import './palette.css';

const RESULTS_DEBOUNCE_MS = 120;
const MAX_ROWS = 5;
const PROVIDER_NAMES = PROVIDERS.map((p) => PROVIDER_NAME[p]);

type Entry =
  | { kind: 'action'; match: ActionMatch }
  | { kind: 'item'; item: ItemSummary; ranges: Array<[number, number]> }
  | { kind: 'task'; task: TaskSummary; ranges: Array<[number, number]> };

interface SubRow {
  key: string;
  label: string;
  icon: IconName;
  hint?: string;
  /** Absent: shown but not runnable (e.g. a provider that is not set up). */
  run?: () => unknown;
}

/** `text` with `<mark>` over `ranges` and the `bold` words in `<b>`. */
export function Highlight({ text, ranges, bold = [] }: { text: string; ranges: Array<[number, number]>; bold?: readonly string[] }): ReactElement {
  const marked = new Uint8Array(text.length);
  for (const [a, b] of ranges) marked.fill(1, a, b);
  for (const word of bold) {
    for (let at = text.indexOf(word); at >= 0; at = text.indexOf(word, at + word.length)) {
      for (let i = at; i < at + word.length; i++) marked[i] |= 2;
    }
  }
  const parts: ReactNode[] = [];
  let start = 0;
  for (let i = 1; i <= text.length; i++) {
    if (i < text.length && marked[i] === marked[start]) continue;
    const s = text.slice(start, i);
    const flag = marked[start];
    const strong = flag & 2 ? <b key={start}>{s}</b> : s;
    parts.push(flag & 1 ? <mark key={start}>{strong}</mark> : strong);
    start = i;
  }
  return <>{parts}</>;
}

export function Palette({ onClose, onSearch }: { onClose: () => void; onSearch: (q: string) => void }): ReactElement {
  const t = useT();
  const { show } = useToast();
  const itemActions = useItemActions();
  const taskActions = useTaskActions();
  const selection = useHomeSelection();
  const { data: integrations } = useIntegrations();
  const petPaused = useSetting<boolean>('pet.paused', false);
  const syncState = useSyncStatus().data?.state;
  const meetings = useMeetingActions();
  const listId = useId();

  const [input, setInput] = useState('');
  const [active, setActive] = useState(0);
  const [sub, setSub] = useState<{ entry: Entry; active: number } | null>(null);
  const query = input.trim();
  const q = useDebounced(query, RESULTS_DEBOUNCE_MS);

  // Focus returns to where it was when the palette closes.
  // Captured before the first commit: autoFocus moves focus before effects run.
  const [before] = useState(() => document.activeElement);
  useEffect(() => {
    return () => {
      if (before instanceof HTMLElement) before.focus();
    };
  }, [before]);

  const items = useQuery({
    queryKey: ['items', 'palette', q],
    queryFn: () => listItems({ view: 'all', search: q, limit: MAX_ROWS }),
    enabled: q !== '',
  });
  const tasks = useQuery({ ...tasksQuery('open', q), enabled: q !== '' });

  const settled = resultsSettled(query, q, [items, tasks]);
  const queryError = items.error ?? tasks.error;
  const fail = (err: unknown) => show({ message: errorMessage(t, err) });
  const ctx: PaletteContext = {
    t,
    query,
    selection,
    integrations: integrations ?? [],
    navigate,
    toast: show,
    setSearch: onSearch,
    async convertToTask(itemId) {
      try {
        await itemActions.convertToTask(await getItem(itemId));
      } catch (err) {
        fail(err);
      }
    },
    petPaused,
    syncState,
    recording: meetings.recording,
    recordMeeting: meetings.record,
    importMeeting: meetings.importFile,
  };

  const actionRows = matchActions(ctx);
  const itemRows = q ? (items.data?.items ?? []) : [];
  const taskRows = q ? (tasks.data ?? []).slice(0, MAX_ROWS) : [];
  const entries: Entry[] = [
    ...actionRows.map((match): Entry => ({ kind: 'action', match })),
    ...itemRows.map((item): Entry => ({ kind: 'item', item, ranges: matchRanges(item.title, query) ?? [] })),
    ...taskRows.map((task): Entry => ({ kind: 'task', task, ranges: matchRanges(task.title, query) ?? [] })),
  ];
  const current = Math.min(active, Math.max(entries.length - 1, 0));

  useEffect(() => setActive(0), [query]);
  useEffect(() => {
    document.getElementById(`${listId}-${sub ? `s${sub.active}` : current}`)?.scrollIntoView({ block: 'nearest' });
  }, [listId, current, sub]);

  /** Closes the palette, then runs `fn`. */
  const runAndClose = (fn: () => unknown) => {
    onClose();
    void Promise.resolve()
      .then(fn)
      .catch(fail);
  };

  const subRows = (entry: Entry): SubRow[] => {
    if (entry.kind === 'item') {
      const id = entry.item.id;
      return [
        { key: 'open', label: t('palette.open'), icon: 'link', run: () => navigate(`item/${encodeURIComponent(id)}`) },
        {
          key: 'copy',
          label: t('common.copy'),
          icon: 'copy',
          run: () => copyItems([id]).then(() => show({ message: t('common.copied') })),
        },
        { key: 'convert', label: t('item.convertToTask'), icon: 'tasks', run: () => ctx.convertToTask(id) },
      ];
    }
    if (entry.kind === 'task') {
      const task = entry.task;
      return [
        { key: 'open', label: t('palette.open'), icon: 'link', run: () => navigate(`tasks/open/${encodeURIComponent(task.id)}`) },
        { key: 'done', label: t('tasks.markDone'), icon: 'check', run: () => taskActions.setDone(task, true) },
        ...PROVIDERS.map((p): SubRow => {
          const ready = isReady(ctx.integrations.find((s) => s.provider === p));
          return {
            key: `export-${p}`,
            label: t('tasks.exportTo', { provider: PROVIDER_NAME[p] }),
            icon: 'export',
            hint: ready ? undefined : t('tasks.setUpFirst'),
            run: ready ? () => exportTask(task.id, p) : undefined,
          };
        }),
      ];
    }
    return [];
  };

  const runEntry = (entry: Entry) => {
    if (entry.kind === 'action') runAndClose(() => entry.match.action.run(ctx));
    else if (entry.kind === 'item') runAndClose(() => navigate(`item/${encodeURIComponent(entry.item.id)}`));
    else runAndClose(() => navigate(`tasks/open/${encodeURIComponent(entry.task.id)}`));
  };

  const onKeyDown = (e: KeyboardEvent) => {
    const rows = sub ? subRows(sub.entry) : entries;
    const at = sub ? sub.active : current;
    const move = (d: number) => {
      if (!rows.length) return;
      const next = (at + d + rows.length) % rows.length;
      if (sub) setSub({ ...sub, active: next });
      else setActive(next);
    };
    switch (e.key) {
      case 'ArrowDown':
        move(1);
        break;
      case 'ArrowUp':
        move(-1);
        break;
      case 'Enter':
        if (e.nativeEvent.isComposing) return;
        if (sub) {
          const run = subRows(sub.entry)[sub.active]?.run;
          if (run) runAndClose(run);
        } else if (entries[current]) runEntry(entries[current]);
        else if (query && settled) runAndClose(() => SEARCH_INBOX.run(ctx));
        break;
      case 'Tab': {
        const entry = entries[current];
        if (!sub && entry && entry.kind !== 'action') setSub({ entry, active: 0 });
        break;
      }
      case 'Escape':
        if (sub) setSub(null);
        else onClose();
        break;
      default:
        return;
    }
    // Consumed here: keep keys from page handlers (e.g. Esc closing the task editor).
    e.preventDefault();
    e.stopPropagation();
  };

  const rowProps = (index: number, selected: boolean, onRun: () => void) => ({
    id: `${listId}-${index}`,
    role: 'option',
    'aria-selected': selected,
    className: `pl-row${selected ? ' is-sel' : ''}`,
    onMouseDown: (e: MouseEvent) => e.preventDefault(),
    onMouseMove: () => !selected && setActive(index),
    onClick: onRun,
  });

  const enterKbd = <Kbd keys={['Enter']} />;
  let body: ReactElement;
  if (sub) {
    const rows = subRows(sub.entry);
    const title = sub.entry.kind === 'item' ? sub.entry.item.title : sub.entry.kind === 'task' ? sub.entry.task.title : '';
    body = (
      <div className="pl-grp" role="group" aria-label={title}>
        <div className="pl-gh pl-ellipsis">{title}</div>
        {rows.map((r, i) => {
          const selected = i === sub.active;
          return (
            <div
              key={r.key}
              id={`${listId}-s${i}`}
              role="option"
              aria-selected={selected}
              aria-disabled={r.run ? undefined : true}
              className={`pl-row${selected ? ' is-sel' : ''}${r.run ? '' : ' is-disabled'}`}
              onMouseDown={(e) => e.preventDefault()}
              onMouseMove={() => !selected && setSub({ ...sub, active: i })}
              onClick={() => r.run && runAndClose(r.run)}
            >
              <span className="pl-ic"><Icon name={r.icon} size={18} /></span>
              <span className="pl-lb"><Highlight text={r.label} ranges={[]} bold={PROVIDER_NAMES} /></span>
              {r.hint ? <span className="pl-cap">{r.hint}</span> : null}
              {selected && r.run ? enterKbd : null}
            </div>
          );
        })}
      </div>
    );
  } else if (!entries.length) {
    body = queryError ? (
      <p className="pl-empty" role="alert">{errorMessage(t, queryError)}</p>
    ) : query && settled ? (
      <p className="pl-empty">{t('palette.empty')}</p>
    ) : (
      <></>
    );
  } else {
    let index = 0;
    const group = (key: string, heading: string, rows: ReactNode[]) =>
      rows.length ? (
        <div key={key} className="pl-grp" role="group" aria-labelledby={`${listId}-${key}`}>
          <div id={`${listId}-${key}`} className="pl-gh">{heading}</div>
          {rows}
        </div>
      ) : null;
    const actionEls = actionRows.map((m) => {
      const i = index++;
      const selected = i === current;
      const hint = m.action.hint?.(t, ctx);
      return (
        <div key={m.action.id} {...rowProps(i, selected, () => runEntry(entries[i]))}>
          <span className="pl-ic"><Icon name={m.action.icon} size={18} /></span>
          <span className="pl-lb"><Highlight text={m.label} ranges={m.ranges} bold={PROVIDER_NAMES} /></span>
          {hint ? <span className="pl-cap">{hint}</span> : null}
          {selected ? enterKbd : null}
        </div>
      );
    });
    const itemEls = itemRows.map((item) => {
      const i = index++;
      const selected = i === current;
      const e = entries[i] as Extract<Entry, { kind: 'item' }>;
      return (
        <div key={item.id} {...rowProps(i, selected, () => runEntry(e))}>
          <span className="pl-ic"><Icon name={KIND_ICON[displayKind(item)]} size={18} /></span>
          <span className="pl-lb"><Highlight text={item.title} ranges={e.ranges} /></span>
          <CategoryTag id={item.categoryId} size="sm" />
          {selected ? enterKbd : null}
        </div>
      );
    });
    const now = Date.now();
    const taskEls = taskRows.map((task) => {
      const i = index++;
      const selected = i === current;
      const e = entries[i] as Extract<Entry, { kind: 'task' }>;
      const pill = task.export ? exportPill(task.export, now, t) : null;
      return (
        <div key={task.id} {...rowProps(i, selected, () => runEntry(e))}>
          <span className="pl-ic"><Icon name="tasks" size={18} /></span>
          <span className="pl-lb"><Highlight text={task.title} ranges={e.ranges} /></span>
          {pill ? <StatusPill status={pill.status} label={pill.label} /> : null}
          {selected ? enterKbd : null}
        </div>
      );
    });
    body = (
      <>
        {group('actions', t('palette.actions'), actionEls)}
        {group('items', t('palette.items'), itemEls)}
        {group('tasks', t('palette.tasks'), taskEls)}
      </>
    );
  }

  const activeId = sub ? `${listId}-s${sub.active}` : entries.length ? `${listId}-${current}` : undefined;
  return (
    <div className="hm-scrim pl-scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="pl" role="dialog" aria-modal="true" aria-label={t('palette.dialog')} onKeyDown={onKeyDown}>
        <div className="pl-head">
          <SearchField
            size="lg"
            className="pl-search"
            placeholder={t('palette.placeholder')}
            autoFocus
            spellCheck={false}
            autoCorrect="off"
            autoCapitalize="off"
            role="combobox"
            aria-expanded
            aria-controls={listId}
            aria-activedescendant={activeId}
            value={input}
            onChange={(e) => {
              setInput(e.currentTarget.value);
              setSub(null);
            }}
          />
        </div>
        <div id={listId} className="pl-list" role="listbox" aria-label={t('palette.dialog')}>
          {body}
        </div>
        <div className="pl-foot">
          <span><Kbd keys={['↑', '↓']} />{t('palette.keys.move')}</span>
          <span><Kbd keys={['Enter']} />{t('palette.keys.run')}</span>
          <span><Kbd keys={['Tab']} />{t('palette.keys.more')}</span>
          <span className="pl-foot__end"><Kbd keys={['Esc']} />{t('palette.keys.close')}</span>
        </div>
      </div>
    </div>
  );
}
