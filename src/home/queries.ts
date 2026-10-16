// Home query options. `items-changed`, `counts-changed` and `settings-changed` refresh them
// (see useLiveQueries in HomeApp).
import { queryOptions } from '@tanstack/react-query';
import { counts, getItem } from '../shared/ipc';

export const countsQuery = queryOptions({ queryKey: ['counts'], queryFn: counts });

export const itemQuery = (id: string) => queryOptions({ queryKey: ['item', id], queryFn: () => getItem(id) });
