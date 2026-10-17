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
import { itemQuery, meetingQuery } from './queries';
import { MeetingActionsProvider } from './meetings/recordFlow';
import { MeetingsPage } from './meetings/MeetingsPage';
import { PetRoomPage } from './petroom/PetRoomPage';
import { navigate, routePath, useRoute, type Route } from './router';
import { SettingsPage } from './settings/SettingsPage';
import { TasksPage } from './tasks/TasksPage';
import { TitleBar } from './TitleBar';
import { SelectionProvider } from './selection';
import { Palette } from './palette/Palette';

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
      // Action items link their tasks.
      void qc.invalidateQueries({ queryKey: ['meeting'] });
    };
    const refreshMeetings = () => {
      void qc.invalidateQueries({ queryKey: ['meetings'] });
      void qc.invalidateQueries({ queryKey: ['meeting'] });
    };
    const subs = [
      on('items-changed', refresh),
      on('tasks-changed', refreshTasks),
      on('exports-changed', refreshTasks),
      on('meetings-changed', refreshMeetings),
      on('recording-status', () => void qc.invalidateQueries({ queryKey: ['meetings'] })),
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
    case 'meetings':
      return 'meetings';
    case 'agents':
      return 'agents';
    case 'categories':
      // `#/categories` edits the first category (see CategoriesPage).
      return route.id ?? firstCategory ?? '';
    case 'integrations':
      return 'integrations';
    case 'pet':
      return 'pet';
    case 'settings':
      return 'settings';
  }
}

export function HomeApp(): ReactElement {
  const t = useT();
  const route = useRoute();
  // The inbox search; only the palette's "Search inbox for …" sets it.
  const [search, setSearch] = useState('');
  const [paletteOpen, setPaletteOpen] = useState(false);
  const lastList = useRef('inbox');
  // Back from an item (e.g. a task's source) returns to the last list, Tasks and Meetings included.
  if (route.page === 'inbox' || route.page === 'category' || route.page === 'meetings' || (route.page === 'tasks' && !route.new)) {
    lastList.current = routePath(route);
  }
  const item = useQuery({ ...itemQuery(route.page === 'item' ? route.id : ''), enabled: route.page === 'item' }).data;
  const meetingId = route.page === 'meetings' ? route.id : undefined;
  const meeting = useQuery({ ...meetingQuery(meetingId ?? ''), enabled: meetingId != null }).data;
  const { data: categories } = useCategories();
  // A category route whose id isn't live shows the Inbox (see InboxPage).
  const category = route.page === 'category' ? categories?.find((c) => c.id === route.categoryId) : undefined;

  useLiveQueries();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setPaletteOpen(true);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  let pageName: string;
  let page: ReactElement;
  switch (route.page) {
    case 'inbox':
      pageName =
        route.view === 'all'
          ? t('home.inbox')
          : `${t('home.inbox')} · ${t(route.view === 'pinned' ? 'common.pinned' : 'common.unsorted')}`;
      page = <InboxPage route={route} search={search} onClearSearch={() => setSearch('')} />;
      break;
    case 'category':
      pageName = category ? categoryLabel(category, t) : t('home.inbox');
      page = <InboxPage route={route} search={search} onClearSearch={() => setSearch('')} />;
      break;
    case 'item':
      pageName = item ? `${categoryLabelOf(categories, item.categoryId, t)} · ${item.title}` : t('common.item');
      page = <ItemDetail key={route.id} id={route.id} mode="page" onBack={() => navigate(lastList.current)} />;
      break;
    case 'tasks':
      pageName = t('tasks.title');
      page = <TasksPage route={route} />;
      break;
    case 'meetings':
      pageName = meeting ? `${t('meetings.title')} · ${meeting.title}` : t('meetings.title');
      page = <MeetingsPage route={route} />;
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
    case 'pet':
      pageName = t('petroom.title');
      page = <PetRoomPage />;
      break;
    case 'settings':
      pageName = t('settings.title');
      page = <SettingsPage section={route.section} onNavigate={navigate} />;
      break;
  }

  return (
    <SelectionProvider>
      <div className="hm-win">
        {/* Inside .hm-win so the consent dialog gets the window's button font and focus ring. */}
        <MeetingActionsProvider>
          <TitleBar page={pageName} onOpenPalette={() => setPaletteOpen(true)} onNavigate={navigate} />
          <div className="hm-body">
            <HomeNav active={navKey(route, item?.categoryId, categories?.[0]?.id)} />
            <main className="hm-main">{page}</main>
          </div>
          {paletteOpen ? <Palette onClose={() => setPaletteOpen(false)} onSearch={setSearch} /> : null}
        </MeetingActionsProvider>
      </div>
    </SelectionProvider>
  );
}
