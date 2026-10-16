// Task mutations shared by the Tasks list and the task editor. Each change shows a toast with
// Undo through the event ids it recorded; failures toast `errorMessage`.
import { useQueryClient } from '@tanstack/react-query';
import { useCallback, useMemo } from 'react';
import { errorMessage } from '../../shared/errors';
import { useT } from '../../shared/i18n';
import { createTask, setTaskDone, snoozeTask, trashTask, undoTaskEvent } from '../../shared/ipc';
import { useToast } from '../../shared/Toast';
import type { NewTask, TaskSummary } from '../../shared/types';

const SNOOZE_MS = 60 * 60 * 1000;

export interface TaskActions {
  setDone(task: TaskSummary, done: boolean): Promise<void>;
  snooze(task: TaskSummary): Promise<void>;
  /** Resolves true once the task is in the trash. */
  trash(task: TaskSummary): Promise<boolean>;
  /** The created task, or null when creating failed. */
  create(input: NewTask): Promise<TaskSummary | null>;
  undo(eventIds: string[]): Promise<void>;
  fail(err: unknown): void;
}

export function useTaskActions(): TaskActions {
  const t = useT();
  const { show } = useToast();
  const qc = useQueryClient();

  const refresh = useCallback(() => {
    void qc.invalidateQueries({ queryKey: ['tasks'] });
    void qc.invalidateQueries({ queryKey: ['task'] });
  }, [qc]);

  const fail = useCallback((err: unknown) => show({ message: errorMessage(t, err) }), [show, t]);

  const undo = useCallback(
    async (eventIds: string[]) => {
      try {
        for (const id of [...eventIds].reverse()) await undoTaskEvent(id);
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
    async (op: () => Promise<{ eventIds: string[] }>, message: string): Promise<boolean> => {
      try {
        const { eventIds } = await op();
        refresh();
        show({ message, undo: eventIds.length ? () => void undo(eventIds) : undefined });
        return true;
      } catch (err) {
        fail(err);
        return false;
      }
    },
    [fail, refresh, show, undo],
  );

  return useMemo<TaskActions>(
    () => ({
      async setDone(task, done) {
        await withUndo(() => setTaskDone(task.id, done), t(done ? 'tasks.markedDone' : 'tasks.reopened'));
      },
      async snooze(task) {
        await withUndo(() => snoozeTask(task.id, Date.now() + SNOOZE_MS), t('tasks.snoozed'));
      },
      trash: (task) => withUndo(() => trashTask(task.id), t('toast.trashed')),
      async create(input) {
        let created: TaskSummary | null = null;
        await withUndo(async () => {
          const res = await createTask(input);
          created = res.task;
          // Undo of a create trashes the task through its `created` event.
          return { eventIds: res.eventIds.slice(0, 1) };
        }, t('tasks.created'));
        return created;
      },
      undo,
      fail,
    }),
    [fail, t, undo, withUndo],
  );
}
