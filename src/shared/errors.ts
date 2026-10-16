// Error text for the UI. Rust commands reject with the `AppError` message string; codes the UI
// switches on (`AppError::Code`) have an `error.<code>` copy key.
import type { I18nKey, TFunction } from './i18n';
import { en } from './i18n/en';

/** The rejection's message: the error code for `AppError::Code`, else the raw reason. */
export function errorText(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

/** `t('error.<code>')` when that key exists, else the raw text. */
export function errorMessage(t: TFunction, e: unknown): string {
  const text = errorText(e);
  const key = `error.${text}`;
  return key in en ? t(key as I18nKey) : text;
}
