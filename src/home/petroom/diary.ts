// Pure helpers for the pet diary panel (tested without a DOM).
import { formatWeekRange } from '../../shared/format';
import type { Lang, TFunction } from '../../shared/i18n';
import type { DiaryDay, DiaryWeek } from '../../shared/types';

export const isEmptyWeek = (w: Pick<DiaryWeek, 'captured' | 'tasksDone' | 'meetings'>): boolean =>
  w.captured + w.tasksDone + w.meetings === 0;

/** offset 0 → `diary.thisWeek`, −1 → `diary.lastWeek`, else `formatWeekRange`. */
export function weekHeading(w: Pick<DiaryWeek, 'offset' | 'start' | 'end'>, t: TFunction, lang: Lang, nowYear: number): string {
  if (w.offset === 0) return t('diary.thisWeek');
  if (w.offset === -1) return t('diary.lastWeek');
  return formatWeekRange(w.start, w.end, lang, nowYear);
}

const pad2 = (n: number) => String(n).padStart(2, '0');

/** Local `YYYY-MM-DD`. */
export function todayIso(now: Date): string {
  return `${now.getFullYear()}-${pad2(now.getMonth() + 1)}-${pad2(now.getDate())}`;
}

export interface DayBar { date: string; isoDay: number; total: number; pct: number; future: boolean }

/** total = captured + tasksDone + meetings; pct = round(total / busiest × 100), 0 when every day is 0;
 *  isoDay = index + 1; future = date > today. */
export function dayBars(days: DiaryDay[], today: string): DayBar[] {
  const totals = days.map((d) => d.captured + d.tasksDone + d.meetings);
  const busiest = Math.max(0, ...totals);
  return days.map((d, i) => ({
    date: d.date,
    isoDay: i + 1,
    total: totals[i],
    pct: busiest ? Math.round((totals[i] / busiest) * 100) : 0,
    future: d.date > today,
  }));
}
