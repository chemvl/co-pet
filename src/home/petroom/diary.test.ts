import { describe, expect, it } from 'vitest';
import { translate, type TFunction } from '../../shared/i18n';
import type { DiaryDay } from '../../shared/types';
import { dayBars, isEmptyWeek, todayIso, weekHeading } from './diary';

const en: TFunction = (key, params) => translate('en', key, params);
const vi: TFunction = (key, params) => translate('vi', key, params);

const day = (date: string, captured: number, tasksDone = 0, meetings = 0): DiaryDay => ({ date, captured, tasksDone, meetings });
const week = (counts: number[]): DiaryDay[] => counts.map((c, i) => day(`2026-10-${String(12 + i).padStart(2, '0')}`, c));

describe('diary', () => {
  it('scales bars to the busiest day', () => {
    const bars = dayBars([day('2026-10-12', 1, 1), day('2026-10-13', 4), day('2026-10-14', 0, 0, 1), ...week([0, 0, 0, 0, 0, 0, 0]).slice(3)], '2026-10-18');
    expect(bars.map((b) => b.pct)).toEqual([50, 100, 25, 0, 0, 0, 0]);
    expect(bars.map((b) => b.total)).toEqual([2, 4, 1, 0, 0, 0, 0]);
    expect(bars.map((b) => b.isoDay)).toEqual([1, 2, 3, 4, 5, 6, 7]);
  });

  it('gives an all-zero week 0 everywhere', () => {
    expect(dayBars(week([0, 0, 0, 0, 0, 0, 0]), '2026-10-18').every((b) => b.pct === 0)).toBe(true);
  });

  it('marks only days after today as future', () => {
    expect(dayBars(week([1, 1, 1, 1, 1, 1, 1]), '2026-10-15').map((b) => b.future)).toEqual([false, false, false, false, true, true, true]);
  });

  it('names the week', () => {
    const w = { start: '2026-10-12', end: '2026-10-18' };
    expect(weekHeading({ ...w, offset: 0 }, en, 'en', 2026)).toBe('This week');
    expect(weekHeading({ ...w, offset: -1 }, vi, 'vi', 2026)).toBe('Tuần trước');
    expect(weekHeading({ ...w, offset: -2 }, en, 'en', 2026)).toBe('12 Oct – 18 Oct');
  });

  it('formats today as a local date', () => {
    expect(todayIso(new Date(2026, 0, 5))).toBe('2026-01-05');
  });

  it('is empty only for zeros', () => {
    expect(isEmptyWeek({ captured: 0, tasksDone: 0, meetings: 0 })).toBe(true);
    expect(isEmptyWeek({ captured: 1, tasksDone: 0, meetings: 0 })).toBe(false);
    expect(isEmptyWeek({ captured: 0, tasksDone: 1, meetings: 0 })).toBe(false);
    expect(isEmptyWeek({ captured: 0, tasksDone: 0, meetings: 1 })).toBe(false);
  });
});
