// The About section's update status line.
import { errorMessage } from '../../shared/errors';
import type { TFunction } from '../../shared/i18n';
import type { UpdateStatus } from '../../shared/types';

/** The status line for `s`; `null` when idle. */
export function updateLine(s: UpdateStatus, t: TFunction): string | null {
  switch (s.state) {
    case 'idle':
      return null;
    case 'checking':
      return t('settings.about.checking');
    case 'downloading':
      return t('settings.about.downloading', { percent: Math.round((s.progress ?? 0) * 100) });
    case 'ready':
      return t('settings.about.ready', { version: s.version ?? '' });
    case 'upToDate':
      return t('settings.about.upToDate');
    case 'failed':
      return errorMessage(t, s.error);
  }
}
