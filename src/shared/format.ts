import type { Lang } from './i18n';

const EN_DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
const VI_DAYS = ['CN', 'T2', 'T3', 'T4', 'T5', 'T6', 'T7'];
const ZH_DAYS = ['周日', '周一', '周二', '周三', '周四', '周五', '周六'];
const ES_DAYS = ['dom', 'lun', 'mar', 'mié', 'jue', 'vie', 'sáb'];
const DAYS: Record<Lang, string[]> = { en: EN_DAYS, vi: VI_DAYS, zh: ZH_DAYS, es: ES_DAYS };
const EN_MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const ES_MONTHS = ['ene', 'feb', 'mar', 'abr', 'may', 'jun', 'jul', 'ago', 'sept', 'oct', 'nov', 'dic'];
/** [minute, hour] units. */
const UNITS: Record<Lang, [string, string]> = { en: ['min', 'h'], vi: ['phút', 'giờ'], zh: ['分钟', '小时'], es: ['min', 'h'] };

const pad2 = (n: number) => String(n).padStart(2, '0');

/** Day and month without a weekday: en `17 Oct`, vi `17/10`, zh `10月17日`, es `17 oct`; `year` appends the year. */
function dayMonth(date: Date, lang: Lang, year = false): string {
  const d = date.getDate();
  const m = date.getMonth();
  const y = date.getFullYear();
  switch (lang) {
    case 'vi': return `${d}/${m + 1}${year ? `/${y}` : ''}`;
    case 'zh': return `${year ? `${y}年` : ''}${m + 1}月${d}日`;
    case 'es': return `${d} ${ES_MONTHS[m]}${year ? ` ${y}` : ''}`;
    case 'en': return `${d} ${EN_MONTHS[m]}${year ? ` ${y}` : ''}`;
  }
}

/** 24-hour local time: `14:00`. */
export function formatTime(d: Date | number): string {
  const date = new Date(d);
  return `${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
}

/** en `Fri 17 Oct`, vi `T6, 17/10`, zh `10月17日 周五`, es `vie 17 oct`. */
export function formatDate(d: Date | number, lang: Lang): string {
  const date = new Date(d);
  const wd = DAYS[lang][date.getDay()];
  switch (lang) {
    case 'vi': return `${wd}, ${dayMonth(date, lang)}`;
    case 'zh': return `${dayMonth(date, lang)} ${wd}`;
    default: return `${wd} ${dayMonth(date, lang)}`;
  }
}

/** Short weekday for an ISO weekday (1 = Monday … 7 = Sunday): en `Mon`, vi `T2`, zh `周一`, es `lun`. */
export function formatWeekday(isoDay: number, lang: Lang): string {
  return DAYS[lang][isoDay % 7];
}

/** Decimal units; one decimal for GB and MB, none for KB; vi and es use a decimal comma. */
export function formatSize(bytes: number, lang: Lang): string {
  const units: [number, string, number][] = [[1e9, 'GB', 1], [1e6, 'MB', 1], [1e3, 'KB', 0]];
  for (const [base, unit, digits] of units) {
    if (bytes >= base) {
      const s = (bytes / base).toFixed(digits);
      return `${lang === 'vi' || lang === 'es' ? s.replace('.', ',') : s} ${unit}`;
    }
  }
  return `${bytes} B`;
}

/** `~10 min` / `~10 phút` / `~10 分钟`; from an hour `~1 h`, `~1 h 30 min` / `~1 giờ 30 phút` / `~1 小时 30 分钟`. */
export function formatEstimate(min: number, lang: Lang): string {
  const [minUnit, hourUnit] = UNITS[lang];
  if (min < 60) return `~${min} ${minUnit}`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  return m ? `~${h} ${hourUnit} ${m} ${minUnit}` : `~${h} ${hourUnit}`;
}

/** Recording time: `04:07`; from an hour `1:02:03`. */
export function formatClock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const mmss = `${pad2(Math.floor((s % 3600) / 60))}:${pad2(s % 60)}`;
  return h ? `${h}:${mmss}` : mmss;
}

/** `42 min` / `42 phút` / `42 分钟`; from an hour `1 h 5 min` / `1 giờ 5 phút`; under a minute `1 min`. */
export function formatDuration(ms: number, lang: Lang): string {
  const [minUnit, hourUnit] = UNITS[lang];
  const total = Math.max(1, Math.round(ms / 60_000));
  const h = Math.floor(total / 60);
  const m = total % 60;
  if (!h) return `${m} ${minUnit}`;
  return m ? `${h} ${hourUnit} ${m} ${minUnit}` : `${h} ${hourUnit}`;
}

/** Joins non-empty parts with ` · ` (source · context · time). */
export function joinMeta(parts: (string | null | undefined | false)[]): string {
  return parts.filter(Boolean).join(' · ');
}

/** en `12 Oct – 18 Oct`, vi `12/10 – 18/10`, zh `10月12日 – 10月18日`, es `12 oct – 18 oct`; a date outside
 *  `nowYear` adds its year (`29 Dec 2025`, `29/12/2025`, `2025年12月29日`, `29 dic 2025`). */
export function formatWeekRange(start: string, end: string, lang: Lang, nowYear: number): string {
  const one = (iso: string) => {
    const [y, m, d] = iso.split('-').map(Number);
    const date = new Date(y, m - 1, d);
    return dayMonth(date, lang, date.getFullYear() !== nowYear);
  };
  return `${one(start)} – ${one(end)}`;
}
