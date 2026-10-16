// Home window shell: title bar, sidebar and the routed page. Keeps every query live from
// Rust events and follows `navigate` events from the pet, tray and menu bar.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef, useState, type ReactElement } from 'react';
import { categoryLabel, categoryLabelOf } from '../shared/categories';
import { useT } from '../shared/i18n';
import { on } from '../shared/ipc';
import { useCategories } from '../shared/queries';
import type { Counts } from '../shared/types';
import { HomeNav, type NavKey } from './HomeNav';
import { InboxPage } from './InboxPage';
import { AgentsPage } from './agents/AgentsPage';
import { CategoriesPage } from './categories/CategoriesPage';
import { ItemDetail } from './ItemDetail';
import { IntegrationsPage } from './integrations/IntegrationsPage';
import { itemQuery } from './queries';
import { navigate, routePath, useRoute, type Route } from './router';
import { SettingsPage } from './settings/SettingsPage';
import { TasksPage } from './tasks/TasksPage';
import { TitleBar } from './TitleBar';
import { SelectionProvider } from './selection';
import { useDebounced } from './hooks';

const SEARCH_DEBOUNCE_MS = 150;

function useLiveQueries(): void {
  const qc = useQueryClient();
  useEffect(() => {
    const refresh = () => {
      void qc.invalidateQueries({ queryKey: ['items'] });
      void qc.invalidateQueries({ queryKey: ['item'] });
      void qc.invalidateQueries({ queryKey: ['counts'] });
    };
    const refreshTasks = () => {
      void qc.invalidateQueries({ queryKey: ['tasks'] });
      void qc.invalidateQueries({ queryKey: ['task'] });
      void qc.invalidateQueries({ queryKey: ['item'] });
      void qc.invalidateQueries({ queryKey: ['next'] });
    };
    const subs = [
      on('items-changed', refresh),
      on('tasks-changed', refreshTasks),
      on('exports-changed', refreshTasks),
      on('settings-changed', refresh),
      on('counts-changed', (c) => qc.setQueryData<Counts>(['counts'], c)),
      on('navigate', ({ route }) => navigate(route)),
    ];
    return () => subs.forEach((s) => void s.then((unlisten) => unlisten()));
  }, [qc]);
}

function navKey(route: Route, itemCategory: string | undefined, firstCategory: string | undefined): NavKey {
  switch (route.page) {
    case 'inbox':
      return route.view === 'unsorted' ? 'unsorted' : 'inbox';
    case 'category':
      return route.categoryId;
    case 'item':
      return itemCategory ?? '';
    case 'tasks':
      return 'tasks';
    case 'agents':
      return 'agents';
    case 'categories':
      // `#/categories` edits the first category (see CategoriesPage).
      return route.id ?? firstCategory ?? '';
    case 'integrations':
      return 'integrations';
    case 'settings':
      return 'settings';
  }
}

export function HomeApp(): ReactElement {
  const t = useT();
  const route = useRoute();
  const [search, setSearch] = useState('');
  const query = useDebounced(search.trim(), SEARCH_DEBOUNCE_MS);
  const searchRef = useRef<HTMLInputElement>(null);
  const lastList = useRef('inbox');
  // Back from an item (e.g. a task's source) returns to the last list, Tasks included.
  if (route.page === 'inbox' || route.page === 'category' || (route.page === 'tasks' && !route.new)) {
    lastList.current = routePath(route);
  }
  const item = useQuery({ ...itemQuery(route.page === 'item' ? route.id : ''), enabled: route.page === 'item' }).data;
  const { data: categories } = useCategories();
  // A category route whose id isn't live shows the Inbox (see InboxPage).
  const category = route.page === 'category' ? categories?.find((c) => c.id === route.categoryId) : undefined;

  useLiveQueries();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const onSearch = (q: string) => {
    setSearch(q);
    if (route.page === 'item' || route.page === 'agents' || route.page === 'categories' || route.page === 'integrations' || route.page === 'settings') {
      navigate(lastList.current);
    }
  };

  let pageName: string;
  let page: ReactElement;
  switch (route.page) {
    case 'inbox':
      pageName =
        route.view === 'all'
          ? t('home.inbox')
          : `${t('home.inbox')} · ${t(route.view === 'pinned' ? 'common.pinned' : 'common.unsorted')}`;
      page = <InboxPage route={route} search={query} onClearSearch={() => setSearch('')} />;
      break;
    case 'category':
      pageName = category ? categoryLabel(category, t) : t('home.inbox');
      page = <InboxPage route={route} search={query} onClearSearch={() => setSearch('')} />;
      break;
    case 'item':
      pageName = item ? `${categoryLabelOf(categories, item.categoryId, t)} · ${item.title}` : t('common.item');
      page = <ItemDetail key={route.id} id={route.id} mode="page" onBack={() => navigate(lastList.current)} />;
      break;
    case 'tasks':
      pageName = t('tasks.title');
      page = <TasksPage route={route} search={query} onClearSearch={() => setSearch('')} />;
      break;
    case 'agents':
      pageName = t('agents.title');
      page = <AgentsPage />;
      break;
    case 'categories':
      pageName = t('categories.title');
      page = <CategoriesPage selectedId={route.id} />;
      break;
    case 'integrations':
      pageName = t('integrations.title');
      page = <IntegrationsPage />;
      break;
    case 'settings':
      pageName = t('settings.title');
      page = <SettingsPage section={route.section} onNavigate={navigate} />;
      break;
  }

  return (
    <SelectionProvider>
      <div className="hm-win">
        <TitleBar ref={searchRef} page={pageName} search={search} onSearch={onSearch} />
        <div className="hm-body">
          <HomeNav active={navKey(route, item?.categoryId, categories?.[0]?.id)} />
          <main className="hm-main">{page}</main>
        </div>
      </div>
    </SelectionProvider>
  );
}
