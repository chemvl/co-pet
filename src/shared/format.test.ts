import { describe, expect, it } from 'vitest';
import {
  formatClock, formatDate, formatDuration, formatEstimate, formatSize, formatTime, formatWeekday, joinMeta,
} from './format';

describe('format', () => {
  it('formats dates per language', () => {
    // Local constructor: the fixture is 17 Oct in every timezone.
    expect(formatDate(new Date(2025, 9, 17), 'vi')).toBe('T6, 17/10');
    expect(formatDate(new Date(2025, 9, 17), 'en')).toBe('Fri 17 Oct');
    expect(formatDate(new Date(2025, 9, 19), 'vi')).toBe('CN, 19/10');
  });
  it('names ISO weekdays per language', () => {
    expect(formatWeekday(1, 'en')).toBe('Mon');
    expect(formatWeekday(7, 'en')).toBe('Sun');
    expect(formatWeekday(1, 'vi')).toBe('T2');
    expect(formatWeekday(7, 'vi')).toBe('CN');
  });
  it('formats 24-hour time', () => {
    expect(formatTime(new Date(2026, 9, 17, 14, 0))).toBe('14:00');
    expect(formatTime(new Date(2026, 9, 17, 9, 5))).toBe('09:05');
  });
  it('formats sizes with locale decimals', () => {
    expect(formatSize(1.9e9, 'vi')).toBe('1,9 GB');
    expect(formatSize(1.9e9, 'en')).toBe('1.9 GB');
    expect(formatSize(2.4e6, 'vi')).toBe('2,4 MB');
    expect(formatSize(12_345, 'en')).toBe('12 KB');
  });
  it('formats estimates', () => {
    expect(formatEstimate(10, 'en')).toBe('~10 min');
    expect(formatEstimate(10, 'vi')).toBe('~10 phút');
    expect(formatEstimate(59, 'en')).toBe('~59 min');
  });
  it('formats estimates from an hour in hours and minutes', () => {
    expect(formatEstimate(60, 'en')).toBe('~1 h');
    expect(formatEstimate(120, 'vi')).toBe('~2 giờ');
    expect(formatEstimate(90, 'en')).toBe('~1 h 30 min');
    expect(formatEstimate(90, 'vi')).toBe('~1 giờ 30 phút');
  });
  it('formats a recording clock', () => {
    expect(formatClock(0)).toBe('00:00');
    expect(formatClock(247_900)).toBe('04:07');
    expect(formatClock(3_599_999)).toBe('59:59');
    expect(formatClock(3_723_000)).toBe('1:02:03');
  });
  it('formats durations per language', () => {
    expect(formatDuration(42 * 60_000, 'en')).toBe('42 min');
    expect(formatDuration(42 * 60_000, 'vi')).toBe('42 phút');
    expect(formatDuration(65 * 60_000, 'en')).toBe('1 h 5 min');
    expect(formatDuration(65 * 60_000, 'vi')).toBe('1 giờ 5 phút');
    expect(formatDuration(120 * 60_000, 'en')).toBe('2 h');
    expect(formatDuration(20_000, 'en')).toBe('1 min');
    expect(formatDuration(20_000, 'vi')).toBe('1 phút');
  });
  it('joins non-empty meta parts', () => {
    expect(joinMeta(['Slack', '', null, '#frontend', undefined, '14:02'])).toBe('Slack · #frontend · 14:02');
  });
});
