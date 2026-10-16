// Settings › Account and sync: which state the section shows and its error copy.
import { errorMessage, errorText } from '../../../shared/errors';
import type { TFunction } from '../../../shared/i18n';
import type { AccountStatus } from '../../../shared/types';

export type AccountView = 'notConfigured' | 'signedOut' | 'needsSetup' | 'needsRecovery' | 'ready' | 'unknown';

/** `notConfigured` comes first, so the section never shows the `notConfigured` error copy. */
export function accountView(s: AccountStatus): AccountView {
  if (!s.configured) return 'notConfigured';
  if (!s.signedIn) return 'signedOut';
  switch (s.keyState) {
    case 'none': return 'signedOut';
    case 'needsSetup': return 'needsSetup';
    case 'needsRecovery': return 'needsRecovery';
    case 'ready': return 'ready';
    case 'unknown': return 'unknown';
  }
}

/** `errorMessage`, except that sign-in throttling reads as too many attempts. */
export function accountError(t: TFunction, e: unknown): string {
  return errorText(e) === 'rateLimited' ? t('settings.account.rateLimited') : errorMessage(t, e);
}
