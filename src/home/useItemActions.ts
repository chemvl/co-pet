// Item mutations shared by the list, the Unsorted queue and Item detail. Each change shows a
// toast with Undo (through the returned event ids); a single url/file move also offers
// "Make it a rule".
import { useQueryClient } from '@tanstack/react-query';
import { useCallback, useMemo } from 'react';
import { categoryLabelOf } from '../shared/categories';
import { useT } from '../shared/i18n';
import { archive, makeRuleFromItem, recategorize, setPinned, trash, undoEvent } from '../shared/ipc';
import { useCategories } from '../shared/queries';
import { useToast } from '../shared/Toast';
import type { CategoryId, ItemSummary } from '../shared/types';
import { ruleTargetOf } from './itemText';

export interface ItemActions {
  moveTo(items: ItemSummary[], categoryId: CategoryId): Promise<void>;
  archive(items: ItemSummary[]): Promise<void>;
  trash(items: ItemSummary[]): Promise<void>;
  togglePin(item: ItemSummary): Promise<void>;
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
      undo,
    }),
    [categories, fail, show, t, undo, withUndo],
  );
}
