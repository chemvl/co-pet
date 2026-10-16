import type { Lang } from './i18n';

const EN_DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
const VI_DAYS = ['CN', 'T2', 'T3', 'T4', 'T5', 'T6', 'T7'];
const EN_MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

const pad2 = (n: number) => String(n).padStart(2, '0');

/** 24-hour local time: `14:00`. */
export function formatTime(d: Date | number): string {
  const date = new Date(d);
  return `${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
}

/** en `Fri 17 Oct`, vi `T6, 17/10`. */
export function formatDate(d: Date | number, lang: Lang): string {
  const date = new Date(d);
  const day = date.getDate();
  const wd = date.getDay();
  if (lang === 'vi') return `${VI_DAYS[wd]}, ${day}/${date.getMonth() + 1}`;
  return `${EN_DAYS[wd]} ${day} ${EN_MONTHS[date.getMonth()]}`;
}

/** Decimal units; one decimal for GB and MB, none for KB; vi uses a decimal comma. */
export function formatSize(bytes: number, lang: Lang): string {
  const units: [number, string, number][] = [[1e9, 'GB', 1], [1e6, 'MB', 1], [1e3, 'KB', 0]];
  for (const [base, unit, digits] of units) {
    if (bytes >= base) {
      const s = (bytes / base).toFixed(digits);
      return `${lang === 'vi' ? s.replace('.', ',') : s} ${unit}`;
    }
  }
  return `${bytes} B`;
}

/** `~10 min` / `~10 phút`; from an hour `~1 h`, `~1 h 30 min` / `~1 giờ`, `~1 giờ 30 phút`. */
export function formatEstimate(min: number, lang: Lang): string {
  const [minUnit, hourUnit] = lang === 'vi' ? ['phút', 'giờ'] : ['min', 'h'];
  if (min < 60) return `~${min} ${minUnit}`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  return m ? `~${h} ${hourUnit} ${m} ${minUnit}` : `~${h} ${hourUnit}`;
}

/** Joins non-empty parts with ` · ` (source · context · time). */
export function joinMeta(parts: (string | null | undefined | false)[]): string {
  return parts.filter(Boolean).join(' · ');
}
