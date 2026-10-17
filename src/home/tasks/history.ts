import type { TFunction } from '../../shared/i18n';
import type { TaskEvent } from '../../shared/types';
import { PROVIDER_NAME } from '../integrations/model';

/** The history line of a task event (without the time). */
export function historyLabel(ev: TaskEvent, t: TFunction): string {
  if (ev.actor === 'meeting' && ev.action === 'created') return t('tasks.history.fromMeeting');
  if (ev.actor === 'notion' || ev.actor === 'asana') {
    const provider = PROVIDER_NAME[ev.actor];
    if (ev.action === 'done') return t('tasks.history.doneIn', { provider });
    if (ev.action === 'reopened') return t('tasks.history.reopenedIn', { provider });
  }
  return t(`tasks.history.${ev.action}`);
}

/** The newest live event of the user or a pulled status: the one History offers Undo for. */
export function undoableEvent(events: readonly TaskEvent[]): TaskEvent | undefined {
  // Events arrive newest first.
  return events.find((e) => (e.actor === 'user' || e.actor === 'notion' || e.actor === 'asana') && e.undoneAt == null);
}
