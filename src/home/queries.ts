// Home query options. `items-changed`, `tasks-changed`, `counts-changed`, `settings-changed`,
// `meetings-changed` and `recording-status` refresh them (see useLiveQueries in HomeApp). The `diary` keys
// refresh on `items-changed`, `tasks-changed` and `meetings-changed`.
import { keepPreviousData, queryOptions } from '@tanstack/react-query';
import { counts, diaryWeek, getItem, getMeeting, getTask, listMeetings, listTasks, whatsNext } from '../shared/ipc';
import type { TaskStatus } from '../shared/types';

export const countsQuery = queryOptions({ queryKey: ['counts'], queryFn: counts });

export const itemQuery = (id: string) => queryOptions({ queryKey: ['item', id], queryFn: () => getItem(id) });

export const tasksQuery = (view: TaskStatus, search = '') =>
  queryOptions({ queryKey: ['tasks', view, search], queryFn: () => listTasks({ view, search: search || undefined }) });

export const taskQuery = (id: string) => queryOptions({ queryKey: ['task', id], queryFn: () => getTask(id) });

export const nextQuery = queryOptions({ queryKey: ['next', 'home'], queryFn: () => whatsNext('home') });

export const meetingsQuery = () => queryOptions({ queryKey: ['meetings'], queryFn: listMeetings });

export const meetingQuery = (id: string) => queryOptions({ queryKey: ['meeting', id], queryFn: () => getMeeting(id) });

export const diaryQuery = (offset: number | null) =>
  queryOptions({ queryKey: ['diary', offset], queryFn: () => diaryWeek(offset), placeholderData: keepPreviousData });
