// Hash router for the Home window: `#/inbox`, `#/inbox/pinned`, `#/inbox/unsorted`,
// `#/category/<id>`, `#/item/<id>`, `#/settings/<section>`.
import { useEffect, useState } from 'react';
import { CATEGORY_IDS, type CategoryId } from '../shared/types';

export type InboxView = 'all' | 'pinned' | 'unsorted';

export type Route =
  | { page: 'inbox'; view: InboxView }
  | { page: 'category'; categoryId: CategoryId }
  | { page: 'item'; id: string }
  | { page: 'settings'; section: string };

const DEFAULT_SETTINGS_SECTION = 'general';

function isCategory(s: string): s is CategoryId {
  return (CATEGORY_IDS as readonly string[]).includes(s);
}

/** Unknown paths fall back to the inbox. `#/category/unsorted` is the Unsorted queue. */
export function parseRoute(hash: string): Route {
  const [head, arg] = hash
    .replace(/^#?\/?/, '')
    .split('/')
    .filter(Boolean)
    .map((p) => decodeURIComponent(p));
  switch (head) {
    case 'inbox':
      return { page: 'inbox', view: arg === 'pinned' || arg === 'unsorted' ? arg : 'all' };
    case 'category':
      if (arg === 'unsorted') return { page: 'inbox', view: 'unsorted' };
      if (arg && isCategory(arg)) return { page: 'category', categoryId: arg };
      break;
    case 'item':
      if (arg) return { page: 'item', id: arg };
      break;
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
