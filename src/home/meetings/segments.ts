// Pure helpers for the transcript: the segment playing at a time, where each proposed task
// change was said, speaker labels and `YYYY-MM-DD` days. Segments are ordered by `startMs`.
import { formatDate } from '../../shared/format';
import type { Lang, TFunction } from '../../shared/i18n';
import type { Speaker, TaskChange, TranscriptSeg } from '../../shared/types';

/** Index of the last segment starting at or before `ms`; -1 before the first. */
export function activeIndex(segs: readonly Pick<TranscriptSeg, 'startMs'>[], ms: number): number {
  let lo = 0;
  let hi = segs.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (segs[mid].startMs <= ms) lo = mid + 1;
    else hi = mid;
  }
  return lo - 1;
}

/** Changes by the index of the segment they were said in (`activeIndex` of `atMs`); changes
 *  without `atMs` or before the first segment are left out. */
export function changeBySegment(
  segs: readonly Pick<TranscriptSeg, 'startMs'>[],
  changes: readonly TaskChange[],
): Map<number, TaskChange[]> {
  const out = new Map<number, TaskChange[]>();
  for (const c of changes) {
    if (c.atMs == null) continue;
    const i = activeIndex(segs, c.atMs);
    if (i < 0) continue;
    const list = out.get(i);
    if (list) list.push(c);
    else out.set(i, [c]);
  }
  return out;
}

/** `sN` → N; `me`, `others` and anything else → 0. */
export function speakerIndex(key: string): number {
  const m = /^s(\d+)$/.exec(key);
  return m ? Number(m[1]) : 0;
}

/** The speaker's name, else "Me", "Others" or "Speaker N". */
export function speakerName(sp: Speaker, t: TFunction): string {
  if (sp.name) return sp.name;
  if (sp.key === 'me') return t('meetings.speaker.me');
  if (sp.key === 'others') return t('meetings.speaker.others');
  return t('meetings.speaker.n', { n: speakerIndex(sp.key) });
}

/** A `YYYY-MM-DD` day as `Fri 17 Oct` / `T6, 17/10`; anything else as given. */
export function formatDay(day: string, lang: Lang): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  return m ? formatDate(new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3])), lang) : day;
}
