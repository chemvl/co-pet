// Due-date grouping and row text for the Tasks screen. Weeks run Monday to Sunday; a date
// without a time is due at 23:59 local.
import { formatDate } from '../../shared/format';
import type { Lang, TFunction } from '../../shared/i18n';
import type { TaskSummary } from '../../shared/types';

export interface TaskGroups {
  overdue: TaskSummary[];
  today: TaskSummary[];
  week: TaskSummary[];
  later: TaskSummary[];
}

export type GroupKey = keyof TaskGroups;

export const GROUP_KEYS: readonly GroupKey[] = ['overdue', 'today', 'week', 'later'];

const DEFAULT_DUE_TIME = '23:59';
/** Same window as the Rust `task_due_soon` rule (`tasks::counts`). */
const DUE_SOON_MS = 30 * 60 * 1000;

/** Local due moment: `dueDate` + (`dueTime` ?? 23:59); null without a date. */
export function dueAt(task: Pick<TaskSummary, 'dueDate' | 'dueTime'>): Date | null {
  if (!task.dueDate) return null;
  const [y, mo, d] = task.dueDate.split('-').map(Number);
  const [h, mi] = (task.dueTime ?? DEFAULT_DUE_TIME).split(':').map(Number);
  return new Date(y, mo - 1, d, h, mi);
}

function startOfDay(d: Date, plusDays = 0): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + plusDays);
}

function compare(a: TaskSummary, b: TaskSummary): number {
  const da = dueAt(a)?.getTime() ?? Infinity;
  const db = dueAt(b)?.getTime() ?? Infinity;
  if (da !== db) return da < db ? -1 : 1;
  if (a.priority !== b.priority) return b.priority - a.priority;
  return a.createdAt - b.createdAt;
}

/**
 * overdue: due before `now`; today: due later today; week: due after today up to this week's
 * Sunday; later: everything else, including tasks without a due date. Each group is sorted by
 * due (undated last), then priority (high first), then creation.
 */
export function groupTasks(tasks: readonly TaskSummary[], now: Date): TaskGroups {
  const tomorrow = startOfDay(now, 1);
  // getDay(): Sunday 0 … Saturday 6, so the Monday after this week is 1–7 days away.
  const nextMonday = startOfDay(now, ((7 - now.getDay()) % 7) + 1);
  const groups: TaskGroups = { overdue: [], today: [], week: [], later: [] };
  for (const task of tasks) {
    const at = dueAt(task);
    if (!at) groups.later.push(task);
    else if (at < now) groups.overdue.push(task);
    else if (at < tomorrow) groups.today.push(task);
    else if (at < nextMonday) groups.week.push(task);
    else groups.later.push(task);
  }
  for (const key of GROUP_KEYS) groups[key].sort(compare);
  return groups;
}

/** Open, not snoozed, with a due time within the next 30 minutes (the pet's due-soon badge rule). */
export function isDueSoon(task: TaskSummary, now: Date): boolean {
  if (task.status !== 'open' || !task.dueTime) return false;
  if (task.snoozedUntil != null && task.snoozedUntil >= now.getTime()) return false;
  const at = dueAt(task);
  return at != null && at.getTime() >= now.getTime() && at.getTime() <= now.getTime() + DUE_SOON_MS;
}

/** Today → `14:00` (or "Today" without a time); other days → `Fri 17 Oct` plus the time if set. */
export function dueText(task: TaskSummary, now: Date, lang: Lang, t: TFunction): string | undefined {
  const at = dueAt(task);
  if (!at) return undefined;
  if (startOfDay(at).getTime() === startOfDay(now).getTime()) return task.dueTime ?? t('tasks.dueToday');
  return task.dueTime ? `${formatDate(at, lang)} ${task.dueTime}` : formatDate(at, lang);
}
