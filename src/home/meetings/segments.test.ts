import { describe, expect, it } from 'vitest';
import { translate, type TFunction } from '../../shared/i18n';
import type { TaskChange } from '../../shared/types';
import { activeIndex, changeBySegment, formatDay, speakerIndex, speakerName } from './segments';

const t: TFunction = (key, params) => translate('en', key, params);
const vi: TFunction = (key, params) => translate('vi', key, params);
const segs = [{ startMs: 1000 }, { startMs: 5000 }, { startMs: 5000 }, { startMs: 9000 }];
const change = (id: string, atMs: number | null): TaskChange => ({
  id, taskId: 't1', field: 'due', from: null, to: '2026-10-23', atMs, state: 'pending', eventIds: [],
});

describe('activeIndex', () => {
  it('is -1 before the first segment and with no segments', () => {
    expect(activeIndex(segs, 0)).toBe(-1);
    expect(activeIndex(segs, 999)).toBe(-1);
    expect(activeIndex([], 5000)).toBe(-1);
  });

  it('picks the last segment starting at or before the time', () => {
    expect(activeIndex(segs, 1000)).toBe(0);
    expect(activeIndex(segs, 4999)).toBe(0);
    expect(activeIndex(segs, 5000)).toBe(2);
    expect(activeIndex(segs, 8999)).toBe(2);
    expect(activeIndex(segs, 60_000)).toBe(3);
  });
});

describe('changeBySegment', () => {
  it('maps each change to the segment it was said in', () => {
    const map = changeBySegment(segs, [change('a', 1500), change('b', 9000), change('c', 1000)]);
    expect([...map.keys()].sort()).toEqual([0, 3]);
    expect(map.get(0)?.map((c) => c.id)).toEqual(['a', 'c']);
    expect(map.get(3)?.map((c) => c.id)).toEqual(['b']);
  });

  it('leaves out changes without a time or before the first segment', () => {
    expect(changeBySegment(segs, [change('a', null), change('b', 500)]).size).toBe(0);
    expect(changeBySegment([], [change('a', 1500)]).size).toBe(0);
  });
});

describe('speakers', () => {
  it('numbers speakers by key', () => {
    expect(speakerIndex('s1')).toBe(1);
    expect(speakerIndex('s12')).toBe(12);
    expect(speakerIndex('others')).toBe(0);
    expect(speakerIndex('me')).toBe(0);
  });

  it('uses the given name, else the default label', () => {
    expect(speakerName({ key: 's2', name: 'Lan' }, t)).toBe('Lan');
    expect(speakerName({ key: 'me', name: null }, t)).toBe('Me');
    expect(speakerName({ key: 'others', name: null }, t)).toBe('Others');
    expect(speakerName({ key: 's3', name: null }, t)).toBe('Speaker 3');
    expect(speakerName({ key: 'me', name: null }, vi)).toBe('Tôi');
    expect(speakerName({ key: 's1', name: null }, vi)).toBe('Người nói 1');
  });
});

describe('formatDay', () => {
  it('formats a YYYY-MM-DD day as a local date and leaves other text alone', () => {
    expect(formatDay('2026-10-23', 'en')).toBe('Fri 23 Oct');
    expect(formatDay('2026-10-23', 'vi')).toBe('T6, 23/10');
    expect(formatDay('next week', 'en')).toBe('next week');
  });
});
