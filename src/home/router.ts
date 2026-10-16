// Hash router for the Home window: `#/inbox`, `#/inbox/pinned`, `#/inbox/unsorted`,
// `#/category/<id>`, `#/item/<id>`, `#/tasks`, `#/tasks/done`, `#/tasks/<open|done>/<id>`,
// `#/tasks/new`, `#/agents`, `#/integrations`, `#/categories`, `#/categories/<id>`, `#/settings/<section>`.
import { useEffect, useState } from 'react';
import type { CategoryId } from '../shared/types';

export type InboxView = 'all' | 'pinned' | 'unsorted';
export type TasksView = 'open' | 'done';

export type Route =
  | { page: 'inbox'; view: InboxView }
  | { page: 'category'; categoryId: CategoryId }
  | { page: 'item'; id: string }
  /** `id` selects a task; `new` opens the editor in create mode. */
  | { page: 'tasks'; view: TasksView; id?: string; new?: true }
  | { page: 'integrations' }
  | { page: 'agents' }
  /** The Categories screen; `id` selects the category being edited (else the first). */
  | { page: 'categories'; id?: string }
  | { page: 'settings'; section: string };

const DEFAULT_SETTINGS_SECTION = 'general';

/**
 * Unknown paths fall back to the inbox. `#/category/unsorted` is the Unsorted queue; any other
 * non-empty `#/category/<id>` is a category page (InboxPage shows the Inbox when it isn't live).
 */
export function parseRoute(hash: string): Route {
  const [head, arg, arg2] = hash
    .replace(/^#?\/?/, '')
    .split('/')
    .filter(Boolean)
    .map((p) => decodeURIComponent(p));
  switch (head) {
    case 'inbox':
      return { page: 'inbox', view: arg === 'pinned' || arg === 'unsorted' ? arg : 'all' };
    case 'category':
      if (arg === 'unsorted') return { page: 'inbox', view: 'unsorted' };
      if (arg) return { page: 'category', categoryId: arg };
      break;
    case 'item':
      if (arg) return { page: 'item', id: arg };
      break;
    case 'tasks':
      if (arg === 'new') return { page: 'tasks', view: 'open', new: true };
      if (arg === 'open' || arg === 'done') return arg2 ? { page: 'tasks', view: arg, id: arg2 } : { page: 'tasks', view: arg };
      return { page: 'tasks', view: 'open' };
    case 'agents':
      return { page: 'agents' };
    case 'categories':
      return arg ? { page: 'categories', id: arg } : { page: 'categories' };
    case 'integrations':
      return { page: 'integrations' };
    case 'settings':
      return { page: 'settings', section: arg || DEFAULT_SETTINGS_SECTION };
  }
  return { page: 'inbox', view: 'all' };
}

/** The route as a path without the leading `#/`. */
export function routePath(r: Route): string {
  switch (r.page) {
    case 'inbox':
      return r.view === 'all' ? 'inbox' : `inbox/${r.view}`;
    case 'category':
      return `category/${r.categoryId}`;
    case 'item':
      return `item/${encodeURIComponent(r.id)}`;
    case 'tasks':
      if (r.new) return 'tasks/new';
      if (r.id) return `tasks/${r.view}/${encodeURIComponent(r.id)}`;
      return r.view === 'open' ? 'tasks' : 'tasks/done';
    case 'agents':
      return 'agents';
    case 'categories':
      return r.id ? `categories/${encodeURIComponent(r.id)}` : 'categories';
    case 'integrations':
      return 'integrations';
    case 'settings':
      return `settings/${r.section}`;
  }
}

/** Accepts `inbox/pinned`, `/inbox/pinned` or `#/inbox/pinned`; an empty path means the inbox. */
export function navigate(path: string): void {
  const hash = `#/${path.replace(/^#?\/?/, '') || 'inbox'}`;
  if (window.location.hash !== hash) window.location.hash = hash;
}

export function useRoute(): Route {
  const [route, setRoute] = useState(() => parseRoute(window.location.hash));
  useEffect(() => {
    const sync = () => setRoute(parseRoute(window.location.hash));
    window.addEventListener('hashchange', sync);
    sync();
    return () => window.removeEventListener('hashchange', sync);
  }, []);
  return route;
}
