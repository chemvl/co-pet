// Inbox (All · Pinned · Unsorted), category pages and the Unsorted queue: header, nudge,
// inline quick note, rows with Load more, selection + bulk bar, J/K/Enter (1/2/E in the
// queue) and the detail pane at ≥1100 px.
import { useInfiniteQuery, useQuery, type InfiniteData } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState, type ReactElement } from 'react';
import { Button, Icon, Kbd, SegmentedControl } from '../ds';
import { categoryLabel, categoryLabelOf } from '../shared/categories';
import { useT } from '../shared/i18n';
import { captureNote, listItems } from '../shared/ipc';
import { useCategories, useSetting } from '../shared/queries';
import { useToast } from '../shared/Toast';
import type { CategoryId, Cursor, ItemPage, ItemSummary } from '../shared/types';
import { isTypingTarget, useDelayed, useMediaQuery } from './hooks';
import { ItemDetail } from './ItemDetail';
import { ItemRow, quickPicks, UnsortedRow } from './ItemRow';
import { categoryMenuItems, MenuButton } from './Menu';
import { countsQuery } from './queries';
import { navigate, type InboxView, type Route } from './router';
import { useItemActions } from './useItemActions';

const PAGE_SIZE = 100;
const SKELETON_DELAY_MS = 400;
const SKELETON_ROWS = 6;

type ListRoute = Extract<Route, { page: 'inbox' } | { page: 'category' }>;
type Sort = 'newest' | 'oldest';
type ListKey = readonly ['items', InboxView, CategoryId | null, string, Sort];

interface InboxPageProps {
  route: ListRoute;
  search: string;
  onClearSearch: () => void;
}

export function InboxPage({ route, search, onClearSearch }: InboxPageProps): ReactElement {
  const t = useT();
  const actions = useItemActions();
  const wide = useMediaQuery('(min-width: 1100px)');
  const { data: categories } = useCategories();
  // A category route whose id isn't live (deleted, mistyped) shows the Inbox.
  const category = route.page === 'category' ? categories?.find((x) => x.id === route.categoryId) : undefined;
  const view: InboxView = route.page === 'inbox' ? route.view : 'all';
  const categoryId = route.page === 'category' && (!categories || category) ? route.categoryId : undefined;
  const queue = view === 'unsorted';
  const [sort, setSort] = useState<Sort>('newest');
  const { data: c } = useQuery(countsQuery);
  const petName = useSetting<string>('pet.name', 'Miso') || 'Miso';

  const list = useInfiniteQuery<ItemPage, Error, InfiniteData<ItemPage, Cursor | null>, ListKey, Cursor | null>({
    queryKey: ['items', view, categoryId ?? null, search, sort],
    queryFn: ({ pageParam }) =>
      listItems({ view, categoryId, search: search || undefined, sort, limit: PAGE_SIZE, cursor: pageParam ?? undefined }),
    initialPageParam: null,
    getNextPageParam: (last) => last.nextCursor ?? undefined,
    // Keep rows on screen while the search or sort of the same list changes.
    placeholderData: (prev, prevQuery) =>
      prevQuery && prevQuery.queryKey[1] === view && prevQuery.queryKey[2] === (categoryId ?? null) ? prev : undefined,
  });
  const items = useMemo(() => list.data?.pages.flatMap((p) => p.items) ?? [], [list.data]);
  const showSkeleton = useDelayed(list.isPending, SKELETON_DELAY_MS);

  const [activeId, setActiveId] = useState<string | null>(null);
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const [noting, setNoting] = useState(false);
  const lastIndex = useRef(0);
  const rows = useRef(new Map<string, HTMLDivElement>());

  const scope = `${view}|${categoryId ?? ''}`;
  useEffect(() => {
    setActiveId(null);
    setSelected(new Set());
    setNoting(false);
    lastIndex.current = 0;
  }, [scope]);

  // When the active row leaves the list (moved, archived), the row now at its place takes over.
  const found = items.findIndex((it) => it.id === activeId);
  const activeIndex = found >= 0 ? found : Math.min(lastIndex.current, items.length - 1);
  const active: ItemSummary | undefined = items[activeIndex];
  useEffect(() => {
    if (activeIndex >= 0) lastIndex.current = activeIndex;
  }, [activeIndex]);
  useEffect(() => {
    if (active) rows.current.get(active.id)?.scrollIntoView({ block: 'nearest' });
  }, [active]);

  const selectedItems = items.filter((it) => selected.has(it.id));
  const clearSelection = () => setSelected(new Set());
  const open = (it: ItemSummary) => navigate(`item/${encodeURIComponent(it.id)}`);
  const pick = (it: ItemSummary, cat: CategoryId) => {
    setActiveId(it.id);
    void actions.moveTo([it], cat);
  };

  // Keys run from the window so they work wherever focus rests on the page.
  const keys = useRef<(e: KeyboardEvent) => void>(() => {});
  keys.current = (e) => {
    if (e.defaultPrevented || isTypingTarget(e.target) || e.metaKey || e.ctrlKey || e.altKey) return;
    const move = (d: number) => {
      const next = items[Math.min(Math.max(activeIndex + d, 0), items.length - 1)];
      if (next) setActiveId(next.id);
    };
    switch (e.key) {
      case 'j':
      case 'J':
        move(1);
        break;
      case 'k':
      case 'K':
        move(-1);
        break;
      case 'Enter':
        if (!active || (e.target as HTMLElement).closest('button, a, [role="menu"]')) return;
        open(active);
        break;
      case 'Escape':
        if (!selected.size) return;
        clearSelection();
        break;
      case '1':
      case '2': {
        const cat = active ? quickPicks(active, categories)[Number(e.key) - 1] : undefined;
        if (!queue || !active || !cat) return;
        pick(active, cat);
        break;
      }
      case 'e':
      case 'E':
        if (!queue || !active) return;
        void actions.archive([active]);
        break;
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

  const total =
    categoryId != null
      ? (c?.byCategory[categoryId] ?? 0)
      : view === 'pinned'
        ? (c?.pinned ?? 0)
        : view === 'unsorted'
          ? (c?.unsorted ?? 0)
          : (c?.inbox ?? 0);
  const title = category ? categoryLabel(category, t) : t('home.inbox');
  const showDetail = wide && !queue && items.length > 0;

  const header = queue ? (
    <div className="hm-bar hm-bar--queue">
      <div className="hm-bar__lead">
        <h1 className="title hm-bar__title">{categoryLabelOf(categories, 'unsorted', t)}</h1>
        <div className="label hm-muted hm-bar__sub">{t('home.unsortedLead', { n: total })}</div>
      </div>
      <div className="hm-keys">
        <Kbd keys={['J', 'K']} />
        {t('home.keys.move')}
        <Kbd keys={['1', '2']} />
        {t('home.keys.pick')}
        <Kbd keys={['E']} />
        {t('home.keys.archive')}
      </div>
    </div>
  ) : (
    <div className="hm-bar">
      <h1 className="title hm-bar__title">{title}</h1>
      <span className="caption hm-muted">{total === 1 ? t('home.itemCountOne') : t('home.itemCount', { n: total })}</span>
      {route.page === 'inbox' ? (
        <SegmentedControl
          className="hm-bar__seg"
          label={t('home.filter')}
          value={view}
          options={[
            { value: 'all', label: t('common.all') },
            { value: 'pinned', label: t('common.pinned'), count: c?.pinned },
            { value: 'unsorted', label: t('common.unsorted'), count: c?.unsorted },
          ]}
          onChange={(v) => navigate(v === 'all' ? 'inbox' : `inbox/${v}`)}
        />
      ) : null}
      <div className="hm-grow" />
      <MenuButton
        variant="quiet"
        size="sm"
        iconEnd="chevron-down"
        align="end"
        menuLabel={t('home.sortNewest')}
        items={(['newest', 'oldest'] as const).map((s) => ({
          key: s,
          label: t(s === 'newest' ? 'home.sortNewest' : 'home.sortOldest'),
          checked: s === sort,
          onSelect: () => setSort(s),
        }))}
      >
        {t(sort === 'newest' ? 'home.sortNewest' : 'home.sortOldest')}
      </MenuButton>
      <Button variant="primary" size="sm" icon="note" onClick={() => setNoting(true)}>
        {t('home.quickNote')}
      </Button>
    </div>
  );

  let body: ReactElement;
  if (list.isError) {
    body = (
      <div className="hm-listmsg">
        <span className="body">{t('home.actionFailed', { reason: String(list.error) })}</span>
        <Button variant="secondary" size="sm" onClick={() => void list.refetch()}>
          {t('common.retry')}
        </Button>
      </div>
    );
  } else if (list.isPending) {
    body = showSkeleton ? <Skeleton label={t('home.loading')} /> : <></>;
  } else if (!items.length) {
    body = search ? (
      <EmptyState
        title={t('home.empty.searchTitle')}
        line={t('home.empty.searchLine', { q: search })}
        action={{ label: t('home.clearSearch'), run: onClearSearch }}
      />
    ) : queue ? (
      <EmptyState
        title={t('home.empty.unsortedTitle')}
        line={t('home.empty.dropLine', { name: petName })}
        action={{ label: t('home.goToInbox'), run: () => navigate('inbox') }}
      />
    ) : view === 'pinned' ? (
      <EmptyState
        title={t('home.empty.pinnedTitle')}
        line={t('home.empty.pinnedLine')}
        action={{ label: t('home.goToInbox'), run: () => navigate('inbox') }}
      />
    ) : (
      <EmptyState
        title={categoryId ? t('home.empty.categoryTitle', { category: title }) : t('home.empty.inboxTitle')}
        line={t('home.empty.dropLine', { name: petName })}
        action={{ label: t('home.writeNote'), run: () => setNoting(true) }}
      />
    );
  } else {
    body = (
      <div
        className="hm-rows"
        role="listbox"
        aria-label={t('home.items')}
        aria-multiselectable
        aria-activedescendant={active ? `row-${active.id}` : undefined}
      >
        {items.map((it) => {
          const rowProps = {
            item: it,
            active: it.id === active?.id,
            checked: selected.has(it.id),
            rowRef: (el: HTMLDivElement | null) => {
              if (el) rows.current.set(it.id, el);
              else rows.current.delete(it.id);
            },
            onToggle: () =>
              setSelected((s) => {
                const next = new Set(s);
                if (!next.delete(it.id)) next.add(it.id);
                return next;
              }),
            onClick: () => {
              setActiveId(it.id);
              if (!wide && !queue) open(it);
            },
            onOpen: () => open(it),
          };
          return queue ? (
            <UnsortedRow key={it.id} {...rowProps} onPick={(cat) => pick(it, cat)} />
          ) : (
            <ItemRow key={it.id} {...rowProps} />
          );
        })}
        {list.hasNextPage ? (
          <div className="hm-more">
            <Button variant="secondary" size="sm" disabled={list.isFetchingNextPage} onClick={() => void list.fetchNextPage()}>
              {t('common.loadMore')}
            </Button>
          </div>
        ) : null}
      </div>
    );
  }

  const listPane = (
    <section className={`hm-list${showDetail ? ' hm-list--split' : ''}${queue ? ' hm-list--queue' : ''}`} aria-label={t('home.items')}>
      {selectedItems.length ? (
        <div className="hm-bulk" role="toolbar" aria-label={t('home.selected', { n: selectedItems.length })}>
          <span className="hm-bulk__count">{t('home.selected', { n: selectedItems.length })}</span>
          <MenuButton
            variant="secondary"
            size="sm"
            iconEnd="chevron-down"
            menuLabel={t('common.moveTo')}
            items={categoryMenuItems(t, categories ?? [], (cat) => {
              void actions.moveTo(selectedItems, cat);
              clearSelection();
            })}
          >
            {t('common.moveTo')}
          </MenuButton>
          <Button
            variant="quiet"
            size="sm"
            icon="trash"
            onClick={() => {
              void actions.archive(selectedItems);
              clearSelection();
            }}
          >
            {t('common.archive')}
          </Button>
          <Button variant="quiet" size="sm" icon="close" aria-label={t('home.clearSelection')} onClick={clearSelection} />
        </div>
      ) : null}
      {!queue && (c?.unsorted ?? 0) > 0 ? (
        <div className="hm-nudge">
          <span className="hm-c-unsorted hm-nudge__icon"><Icon name="unsorted" size={18} /></span>
          <span className="hm-grow">{t('home.nudge', { n: c?.unsorted ?? 0 })}</span>
          <Button variant="secondary" size="sm" onClick={() => navigate('inbox/unsorted')}>
            {t('home.sortNow')}
          </Button>
        </div>
      ) : null}
      {noting ? <QuickNoteRow onDone={() => setNoting(false)} /> : null}
      <div className="hm-list__scroll">{body}</div>
    </section>
  );

  return (
    <div className="hm-page">
      {header}
      <div className="hm-split">
        {listPane}
        {showDetail ? (
          <section className="hm-pane" aria-label={t('home.selectedItem')}>
            {active ? <ItemDetail key={active.id} id={active.id} mode="pane" /> : null}
          </section>
        ) : null}
      </div>
    </div>
  );
}

function QuickNoteRow({ onDone }: { onDone: () => void }): ReactElement {
  const t = useT();
  const { show } = useToast();
  const [text, setText] = useState('');
  const save = () => {
    const note = text.trim();
    if (!note) return;
    captureNote(note)
      .then((res) => {
        if (res.status === 'failed') show({ message: t('home.actionFailed', { reason: res.message ?? '' }) });
      })
      .catch((err: unknown) => show({ message: t('home.actionFailed', { reason: String(err) }) }));
    onDone();
  };
  return (
    <div className="hm-noterow">
      <div className="hm-kt"><Icon name="note" size={18} /></div>
      <input
        className="hm-noterow__input body"
        autoFocus
        aria-label={t('home.quickNote')}
        placeholder={t('home.quickNotePlaceholder')}
        value={text}
        onChange={(e) => setText(e.currentTarget.value)}
        onBlur={() => {
          if (!text.trim()) onDone();
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            save();
          } else if (e.key === 'Escape') {
            e.preventDefault();
            onDone();
          }
        }}
      />
      <Button variant="primary" size="sm" onMouseDown={(e) => e.preventDefault()} onClick={save} disabled={!text.trim()}>
        {t('common.save')}
      </Button>
    </div>
  );
}

export function EmptyState(p: { title: string; line: string; action: { label: string; run: () => void } }): ReactElement {
  return (
    <div className="hm-empty">
      <h2 className="display hm-empty__title">{p.title}</h2>
      <p className="body hm-empty__line">{p.line}</p>
      <Button variant="secondary" onClick={p.action.run}>
        {p.action.label}
      </Button>
    </div>
  );
}

export function Skeleton({ label }: { label: string }): ReactElement {
  return (
    <div className="hm-skeleton" role="status" aria-label={label}>
      {Array.from({ length: SKELETON_ROWS }, (_, i) => (
        <div key={i} className="hm-skeleton__row" aria-hidden>
          <span className="hm-skeleton__tile" />
          <span className="hm-skeleton__lines">
            <span style={{ width: `${70 - (i % 3) * 12}%` }} />
            <span style={{ width: `${88 - (i % 2) * 20}%` }} />
          </span>
        </div>
      ))}
    </div>
  );
}
