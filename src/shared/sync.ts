// Pure mapping from the sync engine's state to what the UI shows: the title-bar pill, whether
// "Sync now" is offered, and which items still carry a local-only pill.
import type { IconName, Status } from '../ds/types';
import type { I18nKey } from './i18n';
import type { ItemSummary, SyncState } from './types';

export interface SyncPill {
  status: Status;
  label: I18nKey;
  icon?: IconName;
}

export function syncPill(state: SyncState): SyncPill {
  switch (state) {
    case 'off':
    case 'paused':
      return { status: 'local', label: 'common.localOnly' };
    case 'needsKey':
      return { status: 'pending', label: 'sync.needsKey' };
    case 'syncing':
      return { status: 'pending', label: 'sync.syncing', icon: 'sync' };
    case 'idle':
      return { status: 'synced', label: 'sync.synced' };
    case 'offline':
      return { status: 'pending', label: 'sync.offline' };
    case 'error':
      return { status: 'failed', label: 'sync.failed' };
    case 'signedOut':
      return { status: 'failed', label: 'sync.signedOut' };
  }
}

/** "Sync now" is offered from the palette in these states. */
export function canSyncNow(state: SyncState | undefined): boolean {
  return state === 'idle' || state === 'offline' || state === 'error';
}

/**
 * The label of an item's `local` pill, or null for none. While sync runs only a file over the size
 * limit stays here ("Only on this Mac"); otherwise `localByDefault` decides ("Local only").
 */
export function localPillLabel(
  item: Pick<ItemSummary, 'fileTooLarge'>,
  state: SyncState | undefined,
  localByDefault: boolean,
): I18nKey | null {
  // Content leaves this Mac in these states.
  if (state === 'idle' || state === 'syncing' || state === 'offline') return item.fileTooLarge ? 'item.onlyThisMac' : null;
  return localByDefault ? 'common.localOnly' : null;
}
