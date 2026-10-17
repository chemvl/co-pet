import { describe, expect, it } from 'vitest';
import { translate, type TFunction } from '../../shared/i18n';
import type { TaskEvent } from '../../shared/types';
import { historyLabel, undoableEvent } from './history';

const en: TFunction = (key, params) => translate('en', key, params);
const vi: TFunction = (key, params) => translate('vi', key, params);

let n = 0;
const ev = (patch: Partial<TaskEvent>): TaskEvent => ({
  id: `e${++n}`,
  at: 1000,
  actor: 'user',
  clientId: null,
  action: 'edited',
  beforeJson: null,
  afterJson: null,
  undoneAt: null,
  ...patch,
});

describe('historyLabel', () => {
  it('names the service for pulled status changes', () => {
    expect(historyLabel(ev({ actor: 'asana', action: 'done' }), en)).toBe('Completed in Asana');
    expect(historyLabel(ev({ actor: 'notion', action: 'reopened' }), vi)).toBe('Đã mở lại trong Notion');
  });

  it('keeps meeting and plain action lines', () => {
    expect(historyLabel(ev({ actor: 'meeting', action: 'created' }), en)).toBe('Added from a meeting');
    expect(historyLabel(ev({ actor: 'user', action: 'edited' }), en)).toBe('Edited');
  });
});

describe('undoableEvent', () => {
  it('skips next/agent and undone events', () => {
    const user = ev({ actor: 'user', action: 'done' });
    const events = [ev({ actor: 'next' }), ev({ actor: 'agent' }), ev({ actor: 'asana', undoneAt: 2000 }), user];
    expect(undoableEvent(events)).toBe(user);
    expect(undoableEvent([ev({ actor: 'agent' })])).toBeUndefined();
  });

  it('returns the newest asana event when it is newest', () => {
    const asana = ev({ actor: 'asana', action: 'done' });
    expect(undoableEvent([asana, ev({ actor: 'user' })])).toBe(asana);
  });
});
