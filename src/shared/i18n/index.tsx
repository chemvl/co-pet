import { createContext, useCallback, useContext, useMemo, type ReactNode } from 'react';
import { platform, type Platform } from '../platform';
import { en, type I18nKey } from './en';
import { es } from './es';
import { vi } from './vi';
import { enWin, esWin, viWin, zhWin } from './win';
import { zh } from './zh';

export const LANGS = ['en', 'vi', 'zh', 'es'] as const;
export type Lang = (typeof LANGS)[number];
export type LangSetting = 'system' | Lang;
export type { I18nKey };
export type TParams = Record<string, string | number>;
export type TFunction = (key: I18nKey, params?: TParams) => string;

const DICTS: Record<Lang, Record<I18nKey, string>> = { en, vi, zh, es };
const WIN: Record<Lang, Partial<Record<I18nKey, string>>> = { en: enWin, vi: viWin, zh: zhWin, es: esWin };

/** Each UI language's name, written in that language, for language pickers. */
export const LANG_LABEL: Record<Lang, I18nKey> = {
  en: 'common.english',
  vi: 'common.vietnamese',
  zh: 'common.chinese',
  es: 'common.spanish',
};

/** The UI language for a stored `ui.language` value; anything unknown is `'system'`. */
export function langSetting(value: unknown): LangSetting {
  return LANGS.find((l) => l === value) ?? 'system';
}

/** `'system'` → the OS language when it is one of `LANGS` (any `zh-*` → `zh`), else `'en'`. */
export function resolveLang(setting: LangSetting): Lang {
  if (setting !== 'system') return setting;
  const nav = typeof navigator !== 'undefined' ? navigator.language.toLowerCase() : '';
  return LANGS.find((l) => nav.startsWith(l)) ?? 'en';
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
