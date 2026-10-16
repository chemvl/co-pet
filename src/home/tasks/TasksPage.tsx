// Tasks (Tasks.dc.html): Open grouped by due date (Overdue · Today · This week · Later) or
// Done, the task editor in the right column (over the list below 1100 px), row actions menu,
// J/K focus, Enter opens, Space toggles done. Rows show their export state; with no task
// selected the right column shows What's Next and the export targets.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useCallback, useEffect, useId, useMemo, useRef, useState, type ReactElement } from 'react';
import { Button, Icon, SegmentedControl, StatusPill, TaskRow } from '../../ds';
import { errorMessage } from '../../shared/errors';
import { formatEstimate } from '../../shared/format';
import { useLang, useT } from '../../shared/i18n';
import { exportTask, nextAction } from '../../shared/ipc';
import { useIntegrations } from '../../shared/queries';
import type { TaskSummary } from '../../shared/types';
import { isTypingTarget, useDelayed, useMediaQuery } from '../hooks';
import { EmptyState, Skeleton } from '../InboxPage';
import { exportPill, isReady, isRetrying, PROVIDER_NAME, PROVIDERS } from '../integrations/model';
import { QueueLine } from '../integrations/parts';
import { Menu, type MenuItem } from '../Menu';
import { countsQuery, nextQuery, tasksQuery } from '../queries';
import { navigate, type Route } from '../router';
import { dueText, GROUP_KEYS, groupTasks, isDueSoon, type GroupKey } from './groups';
import { NextCardView, type CardAction } from '../../shared/NextCardView';
import { TaskEditor } from './TaskEditor';
import { useTaskActions } from './useTaskActions';
import './tasks.css';

const SKELETON_DELAY_MS = 400;
/** Groups and due styling follow the clock between data changes. */
const CLOCK_TICK_MS = 60_000;
const RETRY_TICK_MS = 1000;

type TasksRoute = Extract<Route, { page: 'tasks' }>;

interface TasksPageProps {
  route: TasksRoute;
  search: string;
  onClearSearch: () => void;
}

export function TasksPage({ route, search, onClearSearch }: TasksPageProps): ReactElement {
  const t = useT();
  const lang = useLang();
  const actions = useTaskActions();
  const wide = useMediaQuery('(min-width: 1100px)');
  const { data: c } = useQuery(countsQuery);
  const { view } = route;
  const list = useQuery({
    ...tasksQuery(view, search),
    // Keep rows on screen while the search of the same view changes.
    placeholderData: (prev, prevQuery) => (prevQuery?.queryKey[1] === view ? prev : undefined),
  });
  const showSkeleton = useDelayed(list.isPending, SKELETON_DELAY_MS);

  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), CLOCK_TICK_MS);
    return () => window.clearInterval(id);
  }, []);

  const tasks = useMemo(() => list.data ?? [], [list.data]);
  const { data: integrations } = useIntegrations();

  // "retrying in {s} s" counts down every second while such a row is listed and the window shows.
  const [nowMs, setNowMs] = useState(() => Date.now());
  const retrying = tasks.some((x) => x.export != null && isRetrying(x.export));
  useEffect(() => {
    if (!retrying) return;
    setNowMs(Date.now());
    const id = window.setInterval(() => {
      if (!document.hidden) setNowMs(Date.now());
    }, RETRY_TICK_MS);
    return () => window.clearInterval(id);
  }, [retrying]);

  const groups = useMemo(() => (view === 'open' ? groupTasks(tasks, now) : null), [view, tasks, now]);
  // Row order on screen; J/K and Space index into it.
  const ordered = groups ? GROUP_KEYS.flatMap((k) => groups[k]) : tasks;

  const listPath = view === 'open' ? 'tasks' : 'tasks/done';
  const close = useCallback(() => navigate(listPath), [listPath]);
  const open = (task: TaskSummary) => navigate(`tasks/${view}/${encodeURIComponent(task.id)}`);
  const editing = !!route.new || route.id != null;

  const [menu, setMenu] = useState<{ task: TaskSummary; anchor: HTMLButtonElement } | null>(null);
  const menuAnchor = useMemo(() => ({ current: menu?.anchor ?? null }), [menu]);
  const closeMenu = useCallback(
    (refocus: boolean) => {
      if (refocus) menu?.anchor.focus();
      setMenu(null);
    },
    [menu],
  );

  const listRef = useRef<HTMLElement>(null);
  // Keys run from the window so they work wherever focus rests on the page.
  const keys = useRef<(e: KeyboardEvent) => void>(() => {});
  keys.current = (e) => {
    if (e.defaultPrevented || isTypingTarget(e.target) || e.metaKey || e.ctrlKey || e.altKey) return;
    const rows = [...(listRef.current?.querySelectorAll<HTMLElement>('.cp-task') ?? [])];
    if (!rows.length) return;
    const target = e.target as HTMLElement;
    const at = rows.findIndex((r) => r.contains(target));
    switch (e.key) {
      case 'j':
      case 'J':
      case 'k':
      case 'K': {
        const from = at >= 0 ? at : ordered.findIndex((x) => x.id === route.id);
        const step = e.key.toLowerCase() === 'j' ? 1 : -1;
        rows[from < 0 ? 0 : Math.min(Math.max(from + step, 0), rows.length - 1)].focus();
        break;
      }
      case ' ': {
        // Only on the row itself: the check and the "more" button handle Space natively.
        const task = ordered[at];
        if (!task || rows[at] !== target) return;
        void actions.setDone(task, task.status !== 'done');
        break;
      }
      default:
        return;
    }
    e.preventDefault();
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => keys.current(e);
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const openExported = (url: string) => void openUrl(url).catch(actions.fail);

  const row = (task: TaskSummary, group?: GroupKey) => {
    const pill = task.export ? exportPill(task.export, nowMs, t) : null;
    const url = task.export?.state === 'exported' ? task.export.externalUrl : null;
    return (
      <TaskRow
        key={task.id}
        title={task.title}
        done={task.status === 'done'}
        source={task.sourceTitle ? 'item' : undefined}
        sourceLabel={task.sourceTitle ?? undefined}
        estimate={task.estimateMin != null ? formatEstimate(task.estimateMin, lang) : undefined}
        due={dueText(task, now, lang, t)}
        overdue={group === 'overdue'}
        dueSoon={isDueSoon(task, now)}
        exportStatus={pill?.status}
        exportLabel={pill?.label}
        onExportClick={url ? () => openExported(url) : undefined}
        selected={task.id === route.id}
        labels={{ markDone: t('tasks.markDone'), actions: t('tasks.actions') }}
        onToggle={() => void actions.setDone(task, task.status !== 'done')}
        onOpen={() => open(task)}
        onActions={(anchor) => setMenu((m) => (m?.anchor === anchor ? null : { task, anchor }))}
      />
    );
  };

  /** Per provider: Open in (exported), Retry export (failed), else Export to (queued/exporting: none). */
  const exportItems = (task: TaskSummary): MenuItem[] =>
    PROVIDERS.flatMap((p): MenuItem[] => {
      const provider = PROVIDER_NAME[p];
      const job = task.export?.provider === p ? task.export : null;
      const queue = () => void exportTask(task.id, p).catch(actions.fail);
      if (job?.state === 'exported') {
        const url = job.externalUrl;
        return url ? [{ key: `open-${p}`, label: t('tasks.openIn', { provider }), onSelect: () => openExported(url) }] : [];
      }
      if (job?.state === 'failed') return [{ key: `retry-${p}`, label: t('tasks.retryExport'), onSelect: queue }];
      if (job) return [];
      const ready = isReady(integrations?.find((s) => s.provider === p));
      return [
        {
          key: `export-${p}`,
          label: t('tasks.exportTo', { provider }),
          disabled: !ready,
          hint: ready ? undefined : t('tasks.setUpFirst'),
          onSelect: queue,
        },
      ];
    });

  const menuItems = (task: TaskSummary): MenuItem[] => [
    { key: 'edit', label: t('tasks.edit'), onSelect: () => open(task) },
    ...(task.status === 'open'
      ? [{ key: 'snooze', label: t('tasks.snoozeHour'), onSelect: () => void actions.snooze(task) }]
      : []),
    ...exportItems(task),
    { key: 'trash', label: t('common.moveToTrash'), danger: true, onSelect: () => void actions.trash(task) },
  ];

  const header = (
    <div className="hm-bar">
      <h1 className="title hm-bar__title">{t('tasks.title')}</h1>
      <SegmentedControl
        className="hm-bar__seg"
        label={t('tasks.show')}
        value={view}
        options={[
          { value: 'open', label: t('tasks.openCount', { n: c?.tasksOpen ?? 0 }) },
          { value: 'done', label: t('tasks.done') },
        ]}
        onChange={(v) => navigate(v === 'open' ? 'tasks' : 'tasks/done')}
      />
      <div className="hm-grow" />
      <Button variant="primary" size="sm" icon="plus" onClick={() => navigate('tasks/new')}>
        {t('tasks.new')}
      </Button>
    </div>
  );

  let body: ReactElement;
  if (list.isError) {
    body = (
      <div className="hm-listmsg">
        <span className="body">{errorMessage(t, list.error)}</span>
        <Button variant="secondary" size="sm" onClick={() => void list.refetch()}>
          {t('common.retry')}
        </Button>
      </div>
    );
  } else if (list.isPending) {
    body = showSkeleton ? <Skeleton label={t('tasks.loading')} /> : <></>;
  } else if (!ordered.length) {
    body = search ? (
      <EmptyState
        title={t('home.empty.searchTitle')}
        line={t('tasks.empty.searchLine', { q: search })}
        action={{ label: t('home.clearSearch'), run: onClearSearch }}
      />
    ) : view === 'open' ? (
      <EmptyState
        title={t('tasks.empty.openTitle')}
        line={t('tasks.empty.openLine')}
        action={{ label: t('tasks.new'), run: () => navigate('tasks/new') }}
      />
    ) : (
      <p className="body hm-muted tk-msg">{t('tasks.empty.done')}</p>
    );
  } else if (groups) {
    body = (
      <div className="tk-rows">
        {GROUP_KEYS.filter((k) => groups[k].length).map((k) => (
          <div key={k}>
            <h2 id={`tk-group-${k}`} className={`tk-gh${k === 'overdue' ? ' is-overdue' : ''}`}>
              {k === 'overdue' ? <Icon name="alert" size={16} /> : null}
              {t(`tasks.group.${k}`, { n: groups[k].length })}
            </h2>
            <div role="list" aria-labelledby={`tk-group-${k}`}>
              {groups[k].map((task) => row(task, k))}
            </div>
          </div>
        ))}
      </div>
    );
  } else {
    body = (
      <div className="tk-rows" role="list" aria-label={t('tasks.doneTasks')}>
        {tasks.map((task) => row(task))}
      </div>
    );
  }

  const doneToday = c?.tasksDoneToday ?? 0;
  const footer =
    view === 'open' && !search && doneToday > 0 && list.isSuccess ? (
      <div className="tk-donefoot">
        <Icon name="check" size={16} />
        {t('tasks.doneToday', { n: doneToday })}
        {' · '}
        <button type="button" className="hm-link" onClick={() => navigate('tasks/done')}>
          {t('tasks.showDone')}
        </button>
      </div>
    ) : null;

  const editor = editing ? (
    <TaskEditor
      key={route.new ? 'new' : route.id}
      id={route.new ? undefined : route.id}
      overlay={!wide}
      onClose={close}
      onCreated={(task) => navigate(`tasks/open/${encodeURIComponent(task.id)}`)}
    />
  ) : null;

  return (
    <div className="hm-page">
      {header}
      {editor && !wide ? (
        <div className="tk-overlay">{editor}</div>
      ) : (
        <div className="tk-split">
          <section ref={listRef} className="tk-list" aria-label={t(view === 'open' ? 'tasks.openTasks' : 'tasks.doneTasks')}>
            {body}
            {footer}
          </section>
          {/* Right column: the editor; with nothing selected, What's Next and the export targets. */}
          {wide ? (
            <div className="tk-side">
              {editor ?? (
                <>
                  <NextColumn />
                  <ExportTargets />
                </>
              )}
            </div>
          ) : null}
        </div>
      )}
      {menu ? <Menu anchor={menuAnchor} items={menuItems(menu.task)} label={t('tasks.actions')} align="end" onClose={closeMenu} /> : null}
    </div>
  );
}

/** What's Next for the Tasks page (`whatsNext('home')`: rank only, no model call). */
function NextColumn(): ReactElement | null {
  const t = useT();
  const qc = useQueryClient();
  const { fail } = useTaskActions();
  const next = useQuery(nextQuery);
  if (next.isError) {
    return (
      <div className="hm-listmsg">
        <span className="body">{errorMessage(t, next.error)}</span>
        <Button variant="secondary" size="sm" onClick={() => void next.refetch()}>
          {t('common.retry')}
        </Button>
      </div>
    );
  }
  if (!next.isSuccess) return null;
  const card = next.data;
  if (!card) return <p className="body hm-muted tk-msg">{t('next.empty')}</p>;
  const onAction = (action: CardAction) => {
    nextAction(card.taskId, action, 'home')
      .catch(fail)
      // Skip changes no task, so no `tasks-changed` refreshes the card.
      .finally(() => void qc.invalidateQueries({ queryKey: ['next'] }));
  };
  return <NextCardView card={card} onAction={onAction} />;
}

/** Export targets: one row per provider, its queue line, and the one-way note. */
function ExportTargets(): ReactElement | null {
  const t = useT();
  const headingId = useId();
  const { data: statuses } = useIntegrations();
  if (!statuses) return null;
  return (
    <section className="in-panel" aria-labelledby={headingId}>
      <h2 className="body-strong in-m0" id={headingId}>{t('tasks.exportTargets')}</h2>
      {PROVIDERS.map((p) => {
        const s = statuses.find((x) => x.provider === p);
        const provider = PROVIDER_NAME[p];
        const target = s?.target;
        let pill: ReactElement | null = null;
        if (s?.connected && s.needsReconnect) pill = <StatusPill status="failed" label={t('integrations.reconnectNeeded')} />;
        else if (isReady(s)) pill = <StatusPill status="synced" label={t('integrations.connected')} />;
        return (
          <div key={p} className="in-target">
            <span className="in-logo" aria-hidden="true">{provider[0]}</span>
            <span className="in-target__label">
              {s?.connected && target
                ? t('tasks.exportTarget', { provider, target: target.targetLabel })
                : t('tasks.notSetUp', { provider })}
            </span>
            {pill}
            {isReady(s) ? null : (
              <Button variant="quiet" size="sm" onClick={() => navigate('integrations')}>
                {t('tasks.setUp')}
              </Button>
            )}
          </div>
        );
      })}
      {statuses.map((s) => (
        <QueueLine key={s.provider} status={s} />
      ))}
      <span className="caption in-muted">{t('integrations.oneWay')}</span>
    </section>
  );
}
