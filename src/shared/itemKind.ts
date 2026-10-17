// The kind an item is drawn as: a meeting's item shows the meeting icon, whatever its stored kind.
import type { ItemKind as DsItemKind } from '../ds/types';
import type { ItemKind } from './types';

export function displayKind(item: { kind: ItemKind; meetingId: string | null }): DsItemKind {
  return item.meetingId ? 'meeting' : item.kind;
}
