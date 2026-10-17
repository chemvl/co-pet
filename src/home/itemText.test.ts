import { describe, expect, it } from 'vitest';
import { historyText, rowMeta, whyText } from './itemText';
import { translate, type TFunction } from '../shared/i18n';
import type { ItemDetail, ItemEvent } from '../shared/types';

const t: TFunction = (key, params) => translate('en', key, params);

const item: ItemDetail = {
  id: 'i1',
  kind: 'text',
  title: 'Note',
  summary: null,
  tags: [],
  url: null,
  fileName: null,
  mime: null,
  sizeBytes: null,
  thumbPath: null,
  sourceApp: null,
  sourceKind: 'drop',
  categoryId: 'read',
  triageStatus: 'done',
  triageSource: 'ai',
  triageConfidence: 0.9,
  triageReason: 'agent',
  suggestions: [],
  sensitive: false,
  status: 'inbox',
  pinned: false,
  extractStatus: 'done',
  capturedAt: 0,
  updatedAt: 0,
  addedBy: null,
  taskSuggestion: null,
  meetingId: null,
  fileAvailable: true,
  fileTooLarge: false,
  payloadText: null,
  payloadHtml: null,
  extractedText: null,
  filePath: null,
  events: [],
  tasks: [],
};

const event = (over: Partial<ItemEvent>): ItemEvent => ({
  id: 'e1',
  itemId: 'i1',
  at: 0,
  actor: 'agent',
  clientId: 'client-1',
  action: 'recategorize',
  beforeJson: null,
  afterJson: { category_id: 'read', triage_source: 'ai' },
  undoneAt: null,
  ...over,
});

describe('agent edits', () => {
  it('history names an agent move, not the local model', () => {
    expect(historyText(event({}), item, t, undefined)).toBe('Moved to Read later by an agent');
  });

  it('history shows a tag change', () => {
    expect(historyText(event({ action: 'tag', afterJson: { tags: ['a'] } }), item, t, undefined)).toBe('Tags changed');
  });

  it('the Why chip says an agent moved it, despite the old ai source', () => {
    expect(whyText(item, t, '1 GB')).toEqual({ text: 'Moved by an agent' });
  });
});

describe('meeting items', () => {
  it('the row says "Meeting recording" instead of the source kind', () => {
    const capturedAt = new Date(2026, 9, 15, 10, 0).getTime();
    const note = { ...item, sourceKind: 'note' as const, capturedAt };
    expect(rowMeta(note, t, 'en')).toBe('Quick note · Thu 15 Oct');
    expect(rowMeta({ ...note, meetingId: 'm1' }, t, 'en')).toBe('Meeting recording · Thu 15 Oct');
  });
});
