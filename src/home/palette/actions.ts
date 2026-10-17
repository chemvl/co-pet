// The command palette's action registry, in display order. Each action is pure data plus a
// `run` over the palette context; Palette.tsx filters by `when` and matches labels.
import type { IconName } from '../../ds/types';
import { errorMessage } from '../../shared/errors';
import type { TFunction } from '../../shared/i18n';
import { captureNote, exportTask, retryExports, setSetting, syncNow } from '../../shared/ipc';
import { canSyncNow } from '../../shared/sync';
import type { ShowToast } from '../../shared/Toast';
import type { ExportProvider, ProviderStatus, SyncState } from '../../shared/types';
import { isReady, PROVIDER_NAME, PROVIDERS } from '../integrations/model';
import type { HomeSelection } from '../selection';
import { matchRanges } from './match';

export interface PaletteContext {
  t: TFunction;
  /** The trimmed palette input. */
  query: string;
  selection: HomeSelection;
  integrations: ProviderStatus[];
  navigate: (path: string) => void;
  /** Home's services the actions drive. */
  toast: ShowToast;
  setSearch: (q: string) => void;
  /** Converts the item to a task and opens it; toasts its own errors (never rejects). */
  convertToTask: (itemId: string) => Promise<void>;
  petPaused: boolean;
  /** Undefined until `sync_status` answers. */
  syncState: SyncState | undefined;
  /** A meeting is being recorded (or paused). */
  recording: boolean;
  /** Home's Record meeting flow (consent dialog the first time). */
  recordMeeting: () => void;
  /** Picks an audio or video file and imports it as a meeting. */
  importMeeting: () => void;
}

export interface PaletteAction {
  id: string;
  label: (t: TFunction, ctx: PaletteContext) => string;
  /** The label embeds the query, so it is matched on its text without the query. */
  templated?: boolean;
  icon: IconName;
  keywords?: string[];
  when?: (ctx: PaletteContext) => boolean;
  hint?: (t: TFunction, ctx: PaletteContext) => string | undefined;
  run: (ctx: PaletteContext) => void | Promise<void>;
}

const CARD_SCROLL_FRAMES = 30;

/** Scrolls the Integrations card of `provider` into view once the page has rendered it. */
function scrollToCard(provider: ExportProvider): void {
  let frames = 0;
  const step = () => {
    const card = document.querySelector(`.in-page [data-provider="${provider}"]`);
    if (card) card.scrollIntoView({ block: 'start', behavior: 'smooth' });
    else if (++frames < CARD_SCROLL_FRAMES) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

const status = (ctx: PaletteContext, p: ExportProvider) => ctx.integrations.find((s) => s.provider === p);
const waiting = (s: ProviderStatus | undefined) => (s ? s.queued + s.failed : 0);

async function attempt(ctx: PaletteContext, work: () => Promise<unknown>): Promise<void> {
  try {
    await work();
  } catch (err) {
    ctx.toast({ message: errorMessage(ctx.t, err) });
  }
}

const goTo = (id: string, icon: IconName, key: Parameters<TFunction>[0], path: string): PaletteAction => ({
  id,
  icon,
  label: (t) => t(key),
  run: (ctx) => ctx.navigate(path),
});

export const ACTIONS: readonly PaletteAction[] = [
  goTo('go.inbox', 'inbox', 'palette.goInbox', 'inbox'),
  goTo('go.tasks', 'tasks', 'palette.goTasks', 'tasks'),
  goTo('go.meetings', 'meeting', 'palette.goMeetings', 'meetings'),
  {
    id: 'meeting.record',
    icon: 'mic',
    label: (t) => t('meetings.record'),
    when: (ctx) => !ctx.recording,
    run: (ctx) => ctx.recordMeeting(),
  },
  {
    id: 'meeting.import',
    icon: 'file',
    label: (t) => t('meetings.import'),
    run: (ctx) => ctx.importMeeting(),
  },
  goTo('go.agents', 'agent', 'palette.goAgents', 'agents'),
  goTo('go.integrations', 'export', 'palette.goIntegrations', 'integrations'),
  goTo('go.categories', 'reference', 'palette.goCategories', 'categories'),
  goTo('go.settings', 'settings', 'palette.openSettings', 'settings'),
  goTo('task.new', 'plus', 'palette.newTask', 'tasks/new'),
  {
    id: 'note.add',
    icon: 'note',
    label: (t, ctx) => t('palette.addNote', { query: ctx.query }),
    templated: true,
    when: (ctx) => ctx.query !== '',
    run: (ctx) =>
      attempt(ctx, async () => {
        await captureNote(ctx.query);
        ctx.toast({ message: ctx.t('palette.saved') });
      }),
  },
  {
    id: 'inbox.search',
    icon: 'search',
    label: (t, ctx) => t('palette.searchInbox', { query: ctx.query }),
    templated: true,
    when: (ctx) => ctx.query !== '',
    run: (ctx) => {
      ctx.setSearch(ctx.query);
      ctx.navigate('inbox');
    },
  },
  ...PROVIDERS.map(
    (p): PaletteAction => ({
      id: `export.${p}`,
      icon: 'export',
      label: (t) => t('palette.exportTask', { provider: PROVIDER_NAME[p] }),
      when: (ctx) => !!ctx.selection.taskId && isReady(status(ctx, p)),
      run: (ctx) => {
        const taskId = ctx.selection.taskId;
        if (taskId) return attempt(ctx, () => exportTask(taskId, p));
      },
    }),
  ),
  ...PROVIDERS.map(
    (p): PaletteAction => ({
      id: `integrations.${p}`,
      icon: 'settings',
      label: (t) => t(p === 'notion' ? 'palette.openNotionMapping' : 'palette.openAsanaProject', { provider: PROVIDER_NAME[p] }),
      run: (ctx) => {
        ctx.navigate('integrations');
        scrollToCard(p);
      },
    }),
  ),
  ...PROVIDERS.map(
    (p): PaletteAction => ({
      id: `retry.${p}`,
      icon: 'sync',
      label: (t) => t('palette.retryExports', { provider: PROVIDER_NAME[p] }),
      when: (ctx) => waiting(status(ctx, p)) > 0,
      hint: (t, ctx) => t('palette.waiting', { n: waiting(status(ctx, p)) }),
      run: (ctx) => attempt(ctx, () => retryExports(p)),
    }),
  ),
  {
    id: 'item.convert',
    icon: 'tasks',
    label: (t) => t('item.convertToTask'),
    when: (ctx) => ctx.selection.itemIds.length === 1,
    run: (ctx) => ctx.convertToTask(ctx.selection.itemIds[0]),
  },
  {
    id: 'sync.now',
    icon: 'sync',
    label: (t) => t('sync.now'),
    when: (ctx) => canSyncNow(ctx.syncState),
    run: (ctx) => attempt(ctx, syncNow),
  },
  {
    id: 'pet.pause',
    icon: 'pause',
    label: (t, ctx) => t(ctx.petPaused ? 'pet.menu.resume' : 'pet.menu.pause'),
    run: (ctx) => attempt(ctx, () => setSetting('pet.paused', !ctx.petPaused)),
  },
];

export const MAX_ACTIONS = 6;

export interface ActionMatch {
  action: PaletteAction;
  label: string;
  /** Highlight ranges in `label`; empty for a keyword or templated match. */
  ranges: Array<[number, number]>;
}

/**
 * The actions to list, in registry order, at most `MAX_ACTIONS`: those whose `when` passes and
 * whose label (or a keyword) matches the query. A templated label matches on its text without
 * the query ("Add note “”"), so typing any text does not list it.
 */
export function matchActions(ctx: PaletteContext): ActionMatch[] {
  const out: ActionMatch[] = [];
  for (const action of ACTIONS) {
    if (out.length === MAX_ACTIONS) break;
    if (action.when && !action.when(ctx)) continue;
    const label = action.label(ctx.t, ctx);
    let ranges: Array<[number, number]> | null;
    if (action.templated) {
      ranges = matchRanges(action.label(ctx.t, { ...ctx, query: '' }), ctx.query) && [];
    } else {
      ranges = matchRanges(label, ctx.query);
      if (!ranges && action.keywords?.some((k) => matchRanges(k, ctx.query))) ranges = [];
    }
    if (ranges) out.push({ action, label, ranges });
  }
  return out;
}

/** The action Enter runs when nothing matches. */
export const SEARCH_INBOX = ACTIONS.find((a) => a.id === 'inbox.search')!;
