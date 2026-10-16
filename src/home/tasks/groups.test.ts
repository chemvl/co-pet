import { describe, expect, it } from 'vitest';
import { translate, type TFunction } from '../../shared/i18n';
import type { TaskSummary } from '../../shared/types';
import { dueText, groupTasks, isDueSoon } from './groups';

// Local constructor: Wednesday 15 Oct 2025, 10:00 in every timezone. This week ends Sunday 19 Oct.
const NOW = new Date(2025, 9, 15, 10, 0);

function task(id: string, over: Partial<TaskSummary> = {}): TaskSummary {
  return {
    id,
    title: id,
    status: 'open',
    dueDate: null,
    dueTime: null,
    priority: 0,
    estimateMin: null,
    firstStep: null,
    sourceItemId: null,
    sourceTitle: null,
    categoryId: 'todo',
    addedBy: null,
    snoozedUntil: null,
    doneAt: null,
    createdAt: 0,
    updatedAt: 0,
    export: null,
    ...over,
  };
}

const ids = (ts: TaskSummary[]) => ts.map((t) => t.id);

describe('groupTasks', () => {
  it('treats a date without a time as due at 23:59', () => {
    const g = groupTasks([task('today', { dueDate: '2025-10-15' }), task('yesterday', { dueDate: '2025-10-14' })], NOW);
    expect(ids(g.today)).toEqual(['today']);
    expect(ids(g.overdue)).toEqual(['yesterday']);
    // One minute before the default time it is still due today; one minute after, overdue.
    expect(ids(groupTasks([task('a', { dueDate: '2025-10-15' })], new Date(2025, 9, 15, 23, 58)).today)).toEqual(['a']);
    expect(ids(groupTasks([task('a', { dueDate: '2025-10-15' })], new Date(2025, 9, 16, 0, 0)).overdue)).toEqual(['a']);
  });

  it('ends this week on Sunday', () => {
    const g = groupTasks(
      [
        task('thu', { dueDate: '2025-10-16' }),
        task('sun', { dueDate: '2025-10-19', dueTime: '23:30' }),
        task('mon', { dueDate: '2025-10-20', dueTime: '00:00' }),
      ],
      NOW,
    );
    expect(ids(g.week)).toEqual(['thu', 'sun']);
    expect(ids(g.later)).toEqual(['mon']);
    // On a Sunday nothing is left for "this week" after today.
    const sunday = groupTasks([task('mon', { dueDate: '2025-10-20' })], new Date(2025, 9, 19, 9, 0));
    expect(ids(sunday.week)).toEqual([]);
    expect(ids(sunday.later)).toEqual(['mon']);
  });

  it('puts tasks without a due date in later, after dated ones', () => {
    const g = groupTasks(
      [task('undated', { createdAt: 1 }), task('next-month', { dueDate: '2025-11-20' }), task('undated-high', { priority: 3, createdAt: 2 })],
      NOW,
    );
    expect(ids(g.later)).toEqual(['next-month', 'undated-high', 'undated']);
    expect(g.overdue.length + g.today.length + g.week.length).toBe(0);
  });

  it('marks a time already passed today as overdue', () => {
    const g = groupTasks([task('nine', { dueDate: '2025-10-15', dueTime: '09:59' }), task('eleven', { dueDate: '2025-10-15', dueTime: '11:00' })], NOW);
    expect(ids(g.overdue)).toEqual(['nine']);
    expect(ids(g.today)).toEqual(['eleven']);
  });

  it('sorts by due, then priority, then creation', () => {
    const g = groupTasks(
      [
        task('late', { dueDate: '2025-10-15', dueTime: '17:00' }),
        task('low', { dueDate: '2025-10-15', dueTime: '14:00', priority: 1, createdAt: 1 }),
        task('high', { dueDate: '2025-10-15', dueTime: '14:00', priority: 3, createdAt: 3 }),
        task('low-older', { dueDate: '2025-10-15', dueTime: '14:00', priority: 1, createdAt: 0 }),
      ],
      NOW,
    );
    expect(ids(g.today)).toEqual(['high', 'low-older', 'low', 'late']);
  });
});

describe('isDueSoon', () => {
  it('needs an unsnoozed open task with a time in the next 30 minutes', () => {
    const due = { dueDate: '2025-10-15', dueTime: '10:30' };
    expect(isDueSoon(task('a', due), NOW)).toBe(true);
    expect(isDueSoon(task('a', { ...due, dueTime: '10:31' }), NOW)).toBe(false);
    expect(isDueSoon(task('a', { dueDate: '2025-10-15' }), new Date(2025, 9, 15, 23, 45))).toBe(false);
    expect(isDueSoon(task('a', { ...due, snoozedUntil: NOW.getTime() + 1 }), NOW)).toBe(false);
    expect(isDueSoon(task('a', { ...due, status: 'done' }), NOW)).toBe(false);
  });
});

describe('dueText', () => {
  const t: TFunction = (key, params) => translate('en', key, params);
  it('shows the time today and the date otherwise', () => {
    expect(dueText(task('a', { dueDate: '2025-10-15', dueTime: '14:00' }), NOW, 'en', t)).toBe('14:00');
    expect(dueText(task('a', { dueDate: '2025-10-15' }), NOW, 'en', t)).toBe('Today');
    expect(dueText(task('a', { dueDate: '2025-10-17', dueTime: '09:30' }), NOW, 'en', t)).toBe('Fri 17 Oct 09:30');
    expect(dueText(task('a', { dueDate: '2025-10-17' }), NOW, 'vi', t)).toBe('T6, 17/10');
    expect(dueText(task('a'), NOW, 'en', t)).toBeUndefined();
  });
});
