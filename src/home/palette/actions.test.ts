import { describe, expect, it } from 'vitest';
import { translate, type TFunction } from '../../shared/i18n';
import type { ProviderStatus } from '../../shared/types';
import { matchActions, MAX_ACTIONS, type PaletteContext } from './actions';

const t: TFunction = (key, params) => translate('en', key, params);
const notion: ProviderStatus = {
  provider: 'notion', connected: true, method: 'oauth', accountLabel: 'Me', needsReconnect: false,
  target: { id: 'db', provider: 'notion', accountLabel: 'Me', targetLabel: 'Tasks', config: {} as never }, queued: 0, failed: 1, nextAttemptAt: null, lastError: null,
};
const ctx = (query: string, extra: Partial<PaletteContext> = {}): PaletteContext => ({
  t, query, selection: { itemIds: [] }, integrations: [], navigate: () => {}, toast: () => {},
  setSearch: () => {}, convertToTask: async () => {}, petPaused: false, syncState: undefined, ...extra,
});
const ids = (c: PaletteContext) => matchActions(c).map((m) => m.action.id);

describe('matchActions', () => {
  it('lists the first actions whose `when` passes for an empty query', () => {
    expect(ids(ctx(''))).toEqual(['go.inbox', 'go.tasks', 'go.agents', 'go.integrations', 'go.categories', 'go.settings']);
    expect(ids(ctx('')).length).toBe(MAX_ACTIONS);
  });

  it('lists the Notion rows of the board for "notion" with a task selected', () => {
    const c = ctx('notion', { selection: { taskId: 't1', itemIds: [] }, integrations: [notion] });
    expect(ids(c)).toEqual(['export.notion', 'integrations.notion', 'retry.notion']);
    const [first] = matchActions(c);
    expect(first.label.slice(...first.ranges[0])).toBe('Notion');
  });

  it('matches templated actions on their text without the query', () => {
    expect(ids(ctx('groceries'))).toEqual([]);
    expect(ids(ctx('search'))).toEqual(['inbox.search']);
    const [m] = matchActions(ctx('add note'));
    expect(m.label).toBe('Add note “add note”');
    expect(m.ranges).toEqual([]);
  });

  it('offers Sync now only while sync is idle, offline or failed', () => {
    const offered = (syncState: PaletteContext['syncState']) => ids(ctx('sync', { syncState })).includes('sync.now');
    expect(['idle', 'offline', 'error'].every((s) => offered(s as PaletteContext['syncState']))).toBe(true);
    for (const s of [undefined, 'off', 'paused', 'needsKey', 'signedOut', 'syncing'] as const) expect(offered(s)).toBe(false);
  });
});
