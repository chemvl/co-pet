import { createContext, useCallback, useContext, useMemo, type ReactNode } from 'react';
import { platform, type Platform } from '../platform';
import { en, type I18nKey } from './en';
import { vi } from './vi';
import { enWin, viWin } from './win';

export type Lang = 'en' | 'vi';
export type LangSetting = 'system' | 'en' | 'vi';
export type { I18nKey };
export type TParams = Record<string, string | number>;
export type TFunction = (key: I18nKey, params?: TParams) => string;

const DICTS: Record<Lang, Record<I18nKey, string>> = { en, vi };
const WIN: Record<Lang, Partial<Record<I18nKey, string>>> = { en: enWin, vi: viWin };

/** `'system'` → `'vi'` when the OS language starts with `vi`, else `'en'`. */
export function resolveLang(setting: LangSetting): Lang {
  if (setting === 'en' || setting === 'vi') return setting;
  const nav = typeof navigator !== 'undefined' ? navigator.language : '';
  return nav.toLowerCase().startsWith('vi') ? 'vi' : 'en';
}

/**
 * Looks up `key` in `lang` and replaces `{param}` placeholders. Unknown params stay as written.
 * On Windows, the `win.ts` overlay replaces Mac wording.
 */
export function translate(lang: Lang, key: I18nKey, params?: TParams, os: Platform = platform): string {
  const s = (os === 'windows' ? WIN[lang][key] : undefined) ?? DICTS[lang][key] ?? en[key] ?? key;
  if (!params) return s;
  return s.replace(/\{(\w+)\}/g, (m, k: string) => (k in params ? String(params[k]) : m));
}

const LangContext = createContext<Lang>('en');

export function I18nProvider({ setting, children }: { setting: LangSetting; children: ReactNode }) {
  const lang = useMemo(() => resolveLang(setting), [setting]);
  return <LangContext.Provider value={lang}>{children}</LangContext.Provider>;
}

export function useLang(): Lang {
  return useContext(LangContext);
}

export function useT(): TFunction {
  const lang = useLang();
  return useCallback<TFunction>((key, params) => translate(lang, key, params), [lang]);
}
