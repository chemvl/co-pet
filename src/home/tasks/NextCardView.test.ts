import { describe, expect, it } from 'vitest';
import { translate, type TFunction } from '../../shared/i18n';
import type { NextCard } from '../../shared/types';
import { reasonText } from './NextCardView';

// Local constructor: Wednesday 15 Oct 2025, 10:00 in every timezone.
const NOW = new Date(2025, 9, 15, 10, 0);
const en: TFunction = (key, params) => translate('en', key, params);
const vi: TFunction = (key, params) => translate('vi', key, params);

function card(over: Partial<NextCard> = {}): NextCard {
  return {
    taskId: 't1',
    title: 'Send invoice',
    reason: null,
    reasonKey: null,
    firstStep: null,
    estimateMin: null,
    dueDate: null,
    dueTime: null,
    categoryId: 'todo',
    sourceTitle: null,
    createdAt: new Date(2025, 9, 10).getTime(),
    pickedBy: 'rank',
    ...over,
  };
}

describe('reasonText', () => {
  it('prefers the AI reason', () => {
    expect(reasonText(card({ reason: 'Blocks the release', reasonKey: 'overdue' }), NOW, 'en', en)).toBe('Blocks the release');
  });
  it('has no reason without a key', () => {
    expect(reasonText(card(), NOW, 'en', en)).toBeUndefined();
  });
  it('words the ranker keys', () => {
    expect(reasonText(card({ reasonKey: 'dueSoon', dueDate: '2025-10-15', dueTime: '11:00' }), NOW, 'en', en)).toBe('Due at 11:00');
    expect(reasonText(card({ reasonKey: 'dueThisWeek', dueDate: '2025-10-17' }), NOW, 'en', en)).toBe('Due Fri 17 Oct');
    expect(reasonText(card({ reasonKey: 'waiting' }), NOW, 'vi', vi)).toBe('Đang chờ từ T6, 10/10');
    expect(reasonText(card({ reasonKey: 'highPriority' }), NOW, 'en', en)).toBe('High priority');
    expect(reasonText(card({ reasonKey: 'overdue', dueDate: '2025-10-14' }), NOW, 'en', en)).toMatch(/^Overdue since /);
  });
});
