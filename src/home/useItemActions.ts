// Item mutations shared by the list, the Unsorted queue and Item detail. Each change shows a
// toast with Undo (through the returned event ids); a single url/file move also offers
// "Make it a rule". Convert to task creates a task from the item and opens it on Tasks.
import { useQueryClient } from '@tanstack/react-query';
import { useCallback, useMemo } from 'react';
import { categoryLabelOf } from '../shared/categories';
import { errorMessage } from '../shared/errors';
import { useT } from '../shared/i18n';
import { archive, createTask, makeRuleFromItem, recategorize, setPinned, trash, undoEvent, undoTaskEvent } from '../shared/ipc';
import { useCategories } from '../shared/queries';
import { useToast } from '../shared/Toast';
import type { CategoryId, ItemSummary } from '../shared/types';
import { ruleTargetOf } from './itemText';
import { navigate } from './router';

export interface ItemActions {
  moveTo(items: ItemSummary[], categoryId: CategoryId): Promise<void>;
  archive(items: ItemSummary[]): Promise<void>;
  trash(items: ItemSummary[]): Promise<void>;
  togglePin(item: ItemSummary): Promise<void>;
  /** Creates a task titled with the item's task suggestion (else its title) and opens it. */
  convertToTask(item: ItemSummary): Promise<void>;
  undo(eventIds: string[]): Promise<void>;
}

export function useItemActions(): ItemActions {
  const t = useT();
  const { show } = useToast();
  const qc = useQueryClient();
  const { data: categories } = useCategories();

  const refresh = useCallback(() => {
    void qc.invalidateQueries({ queryKey: ['items'] });
    void qc.invalidateQueries({ queryKey: ['item'] });
    void qc.invalidateQueries({ queryKey: ['counts'] });
  }, [qc]);

  const fail = useCallback(
    (err: unknown) => show({ message: t('home.actionFailed', { reason: String(err) }) }),
    [show, t],
  );

  const undo = useCallback(
    async (eventIds: string[]) => {
      try {
        // Newest first, so several changes to one item unwind in order.
        for (const id of [...eventIds].reverse()) await undoEvent(id);
      } catch (err) {
        fail(err);
      } finally {
        refresh();
      }
    },
    [fail, refresh],
  );

  /** Runs `op`, then toasts `message` with Undo for the events it recorded. */
  const withUndo = useCallback(
    async (op: () => Promise<{ eventIds: string[] }>, message: string, action?: { label: string; run: () => void }) => {
      try {
        const { eventIds } = await op();
        refresh();
        show({ message, action, undo: eventIds.length ? () => void undo(eventIds) : undefined });
      } catch (err) {
        fail(err);
      }
    },
    [fail, refresh, show, undo],
  );

  return useMemo<ItemActions>(
    () => ({
      async moveTo(items, categoryId) {
        if (!items.length) return;
        const category = categoryLabelOf(categories, categoryId, t);
        let message = t('toast.movedTo', { category });
        let action: { label: string; run: () => void } | undefined;
        const target = items.length === 1 && categoryId !== 'unsorted' ? ruleTargetOf(items[0]) : null;
        if (target) {
          message += ` ${
            target.kind === 'domain'
              ? t('toast.ruleDomain', { domain: target.value, category })
              : t('toast.ruleExtension', { ext: target.value, category })
          }`;
          const itemId = items[0].id;
          action = {
            label: t('toast.makeRule'),
            run: () => {
              makeRuleFromItem(itemId)
                .then(() => show({ message: t('home.ruleSaved', { category }) }))
                .catch(fail);
            },
          };
        }
        await withUndo(() => recategorize(items.map((i) => i.id), categoryId), message, action);
      },
      archive: (items) => withUndo(() => archive(items.map((i) => i.id)), t('toast.archived')),
      trash: (items) => withUndo(() => trash(items.map((i) => i.id)), t('toast.trashed')),
      togglePin: (item) =>
        withUndo(() => setPinned(item.id, !item.pinned), t(item.pinned ? 'toast.unpinned' : 'toast.pinned')),
      async convertToTask(item) {
        const refreshTasks = () => {
          void qc.invalidateQueries({ queryKey: ['tasks'] });
          void qc.invalidateQueries({ queryKey: ['task'] });
          void qc.invalidateQueries({ queryKey: ['item', item.id] });
        };
        try {
          const { task, eventIds } = await createTask({ title: item.taskSuggestion ?? item.title, sourceItemId: item.id });
          refreshTasks();
          navigate(`tasks/open/${encodeURIComponent(task.id)}`);
          // Undo trashes the task through its `created` event.
          const created = eventIds[0];
          show({
            message: t('tasks.created'),
            undo: created
              ? () => void undoTaskEvent(created).catch((err) => show({ message: errorMessage(t, err) })).finally(refreshTasks)
              : undefined,
          });
        } catch (err) {
          show({ message: errorMessage(t, err) });
        }
      },
      undo,
    }),
    [categories, fail, qc, show, t, undo, withUndo],
  );
}
