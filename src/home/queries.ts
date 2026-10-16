// Home query options. `items-changed`, `tasks-changed`, `counts-changed` and `settings-changed`
// refresh them (see useLiveQueries in HomeApp).
import { queryOptions } from '@tanstack/react-query';
import { counts, getItem, getTask, listTasks, whatsNext } from '../shared/ipc';
import type { TaskStatus } from '../shared/types';

export const countsQuery = queryOptions({ queryKey: ['counts'], queryFn: counts });

export const itemQuery = (id: string) => queryOptions({ queryKey: ['item', id], queryFn: () => getItem(id) });

export const tasksQuery = (view: TaskStatus, search: string) =>
  queryOptions({ queryKey: ['tasks', view, search], queryFn: () => listTasks({ view, search: search || undefined }) });

export const taskQuery = (id: string) => queryOptions({ queryKey: ['task', id], queryFn: () => getTask(id) });

export const nextQuery = queryOptions({ queryKey: ['next', 'home'], queryFn: () => whatsNext('home') });
