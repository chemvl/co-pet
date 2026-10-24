import { describe, expect, test } from 'vitest';
import { en, type I18nKey } from './en';
import { LANGS, translate, type Lang } from './index';

const MAC_WORDING = /\bMacs?\b|macOS|Finder|System Settings|[Kk]eychain|[Mm]áy Mac|Cài đặt [Hh]ệ thống|访达|系统设置|钥匙串|Ajustes del Sistema|[Ll]lavero|[⌘⌥⌃]/;

// Rendered only on macOS, so they keep their Mac wording.
const MAC_ONLY: Partial<Record<I18nKey, true>> = {
  'settings.whatsnext.calendarUseDesc': true,
  'settings.whatsnext.calendarAsk': true,
  'settings.whatsnext.calendarDenied': true,
  'settings.whatsnext.calendarsEmpty': true,
  'settings.calls.olderMac': true,
  'onboarding.perm.body': true,
};

describe('Windows wording overlay', () => {
  test.each<Lang>(LANGS)('no Mac wording on Windows in %s', (lang) => {
    const leaks = (Object.keys(en) as I18nKey[])
      .filter((key) => !MAC_ONLY[key])
      .filter((key) => MAC_WORDING.test(translate(lang, key, undefined, 'windows')));
    expect(leaks).toEqual([]);
  });

  test('macOS keeps the Mac wording', () => {
    expect(translate('en', 'settings.triage.recommended', undefined, 'macos')).toBe('Recommended for this Mac');
    expect(translate('en', 'settings.triage.recommended', undefined, 'windows')).toBe('Recommended for this PC');
  });

  test('vi says máy tính này on Windows', () => {
    expect(translate('vi', 'error.audioGone', undefined, 'windows')).toContain('máy tính này');
  });
});
