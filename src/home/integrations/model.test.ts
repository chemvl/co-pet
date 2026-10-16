import { describe, expect, it } from 'vitest';
import { translate, type TFunction } from '../../shared/i18n';
import type { NotionSchema, ProviderStatus, TaskExportState } from '../../shared/types';
import { exportPill, isReady, queueLine, reconcileMapping, retryInSeconds, statusMapFor } from './model';

const t: TFunction = (key, params) => translate('en', key, params);
const vi: TFunction = (key, params) => translate('vi', key, params);

const job = (patch: Partial<TaskExportState>): TaskExportState => ({
  provider: 'notion',
  state: 'queued',
  error: null,
  externalUrl: null,
  nextAttemptAt: null,
  ...patch,
});

const status = (patch: Partial<ProviderStatus>): ProviderStatus => ({
  provider: 'notion',
  connected: true,
  method: 'token',
  accountLabel: 'Team',
  needsReconnect: false,
  target: null,
  queued: 0,
  failed: 0,
  nextAttemptAt: null,
  lastError: null,
  ...patch,
});

describe('export pill', () => {
  it('maps each state', () => {
    expect(exportPill(job({}), 0, t)).toEqual({ status: 'pending', label: 'Queued' });
    expect(exportPill(job({ state: 'exporting' }), 0, t)).toEqual({ status: 'exporting', label: 'Exporting…' });
    expect(exportPill(job({ state: 'exported', provider: 'asana' }), 0, t)).toEqual({ status: 'exported', label: 'Asana' });
    expect(exportPill(job({ state: 'failed', error: 'invalid' }), 0, t)).toEqual({ status: 'failed', label: 'Export failed' });
  });

  it('counts down a retry in whole seconds', () => {
    const e = job({ error: 'rateLimited', nextAttemptAt: 30_000 });
    expect(exportPill(e, 0, t)).toEqual({ status: 'due', label: 'Notion · retrying in 30 s' });
    expect(exportPill(e, 500, t).label).toBe('Notion · retrying in 30 s');
    expect(exportPill(e, 29_001, t).label).toBe('Notion · retrying in 1 s');
    expect(exportPill(job({ error: 'network', nextAttemptAt: 3_000, provider: 'asana' }), 0, vi).label).toBe(
      'Asana · thử lại sau 3 giây',
    );
    // Due (or paused while local-only): plain queued, never "0 s".
    expect(exportPill(e, 30_000, t)).toEqual({ status: 'pending', label: 'Queued' });
  });

  it('retryInSeconds rounds up and floors at zero', () => {
    expect(retryInSeconds(null, 5)).toBe(0);
    expect(retryInSeconds(1_001, 0)).toBe(2);
    expect(retryInSeconds(1_000, 0)).toBe(1);
    expect(retryInSeconds(1_000, 9_000)).toBe(0);
  });
});

describe('queue line', () => {
  it('is empty when nothing waits', () => {
    expect(queueLine(status({}), t)).toBeNull();
  });

  it('shows only the count while jobs wait without an error', () => {
    expect(queueLine(status({ queued: 1 }), t)).toBe('1 export waiting');
    expect(queueLine(status({ queued: 2 }), vi)).toBe('2 việc đang chờ xuất');
    expect(queueLine(status({ queued: 1, needsReconnect: true }), t)).toBe('1 export waiting · Reconnect needed');
  });

  it('names the reason from the last error', () => {
    expect(queueLine(status({ queued: 1, lastError: 'rateLimited' }), t)).toBe('1 export waiting · Notion is rate-limiting');
    expect(queueLine(status({ queued: 2, failed: 1, lastError: 'network', provider: 'asana' }), t)).toBe(
      "3 exports waiting · Couldn't reach Asana",
    );
    expect(queueLine(status({ failed: 1, lastError: 'unauthorized' }), t)).toBe('1 export waiting · Reconnect needed');
    expect(queueLine(status({ failed: 1, lastError: 'disconnected' }), t)).toBe('1 export waiting · Reconnect needed');
    expect(queueLine(status({ failed: 1, lastError: 'targetMissing' }), t)).toBe('1 export waiting · Target is gone');
    expect(queueLine(status({ failed: 1, lastError: 'invalid' }), t)).toBe('1 export waiting · Export failed');
  });

  it('isReady needs a connection, no reconnect and a target', () => {
    const target = { id: 'notion', provider: 'notion', accountLabel: 'Team', targetLabel: 'Sprint', config: { workspaceGid: 'w', projectGid: 'p', sectionGid: null } } as const;
    expect(isReady(undefined)).toBe(false);
    expect(isReady(status({}))).toBe(false);
    expect(isReady(status({ target }))).toBe(true);
    expect(isReady(status({ target, needsReconnect: true }))).toBe(false);
    expect(isReady(status({ target, connected: false }))).toBe(false);
  });
});

describe('notion mapping', () => {
  const schema: NotionSchema = {
    title: 'Sprint',
    props: [
      { name: 'Name', kind: 'title', options: [] },
      { name: 'Status', kind: 'status', options: ['Not started', 'In progress', 'Done'] },
      { name: 'Stage', kind: 'select', options: ['New', 'Doing', 'Shipped'] },
      { name: 'Empty', kind: 'select', options: [] },
      { name: 'Due', kind: 'date', options: [] },
    ],
    suggested: {
      title: 'Name',
      status: { prop: 'Status', kind: 'status', todo: 'Not started', done: 'Done' },
      due: 'Due',
      tags: null,
      source: null,
    },
  };

  it('statusMapFor uses the suggestion, else first and last options', () => {
    expect(statusMapFor(schema, 'Status')).toEqual(schema.suggested.status);
    expect(statusMapFor(schema, 'Stage')).toEqual({ prop: 'Stage', kind: 'select', todo: 'New', done: 'Shipped' });
    expect(statusMapFor(schema, 'Empty')).toBeNull();
    expect(statusMapFor(schema, 'Due')).toBeNull();
    expect(statusMapFor(schema, null)).toBeNull();
  });

  it('reconcileMapping drops names the schema no longer has', () => {
    const m = reconcileMapping(schema, {
      title: 'Old title',
      status: { prop: 'Gone', kind: 'select', todo: 'a', done: 'b' },
      due: 'Due',
      tags: 'Tags',
      source: 'Name',
    });
    expect(m).toEqual({ title: 'Name', status: null, due: 'Due', tags: null, source: null });
  });
});
