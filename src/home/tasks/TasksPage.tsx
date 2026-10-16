// Tasks (Tasks.dc.html): Open grouped by due date (Overdue · Today · This week · Later) or
// Done, the task editor in the right column (over the list below 1100 px), row actions menu,
// J/K focus, Enter opens, Space toggles done.
import { useQuery } from '@tanstack/react-query';
import { useCallback, useEffect, useMemo, useRef, useState, type ReactElement } from 'react';
import { Button, Icon, SegmentedControl, TaskRow } from '../../ds';
import { errorMessage } from '../../shared/errors';
import { formatEstimate } from '../../shared/format';
import { useLang, useT } from '../../shared/i18n';
import type { TaskSummary } from '../../shared/types';
import { isTypingTarget, useDelayed, useMediaQuery } from '../hooks';
import { EmptyState, Skeleton } from '../InboxPage';
import { Menu, type MenuItem } from '../Menu';
import { countsQuery, tasksQuery } from '../queries';
import { navigate, type Route } from '../router';
import { dueText, GROUP_KEYS, groupTasks, isDueSoon, type GroupKey } from './groups';
import { TaskEditor } from './TaskEditor';
import { useTaskActions } from './useTaskActions';
import './tasks.css';

const SKELETON_DELAY_MS = 400;
/** Groups and due styling follow the clock between data changes. */
const CLOCK_TICK_MS = 60_000;

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

  const row = (task: TaskSummary, group?: GroupKey) => (
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
      selected={task.id === route.id}
      labels={{ markDone: t('tasks.markDone'), actions: t('tasks.actions') }}
      onToggle={() => void actions.setDone(task, task.status !== 'done')}
      onOpen={() => open(task)}
      onActions={(anchor) => setMenu((m) => (m?.anchor === anchor ? null : { task, anchor }))}
    />
  );

  const menuItems = (task: TaskSummary): MenuItem[] => [
    { key: 'edit', label: t('tasks.edit'), onSelect: () => open(task) },
    ...(task.status === 'open'
      ? [{ key: 'snooze', label: t('tasks.snoozeHour'), onSelect: () => void actions.snooze(task) }]
      : []),
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
          {/* Right column: the editor; with nothing selected, the panels later milestones add. */}
          {wide ? <div className="tk-side">{editor}</div> : null}
        </div>
      )}
      {menu ? <Menu anchor={menuAnchor} items={menuItems(menu.task)} label={t('tasks.actions')} align="end" onClose={closeMenu} /> : null}
    </div>
  );
}
