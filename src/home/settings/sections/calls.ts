import type { TFunction } from '../../../shared/i18n';
import type { CallApp, CallDetectionStatus } from '../../../shared/types';

const APP_NAMES: Record<CallApp, string> = { zoom: 'Zoom', meet: 'Google Meet' };

/** The one status line under the call settings: an active call, then missing permission, then an
 *  older Mac. `needsPermission` shows the Open Settings button. Null when there's nothing to say. */
export function callsStatusLine(
  status: CallDetectionStatus,
  t: TFunction,
): { text: string; needsPermission: boolean } | null {
  if (status.active) return { text: t('settings.calls.active', { app: APP_NAMES[status.active.app] }), needsPermission: false };
  if (status.titles === 'none') return { text: t('settings.calls.meetNeedsPermission'), needsPermission: true };
  if (!status.perProcessMic) return { text: t('settings.calls.olderMac'), needsPermission: false };
  return null;
}
