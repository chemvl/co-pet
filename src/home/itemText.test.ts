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

describe('cloud providers', () => {
  it('cloud reasons name the provider', () => {
    const cloud = { ...item, triageReason: 'ai:anthropic' };
    expect(whyText(cloud, t, '1 GB')).toEqual({ text: 'Sorted by Anthropic', confidence: 0.9, provider: 'Anthropic' });
    expect(whyText({ ...cloud, categoryId: 'unsorted' }, t, '1 GB')?.text).toBe("Anthropic wasn't sure. Pick one of its suggestions.");
    expect(whyText({ ...item, triageSource: 'none', triageReason: 'none:ai-error:lmstudio' }, t, '1 GB')).toEqual({
      text: "LM Studio couldn't sort this one. Pick a category.",
    });
    const ev = event({ actor: 'triage', afterJson: { category_id: 'read', triage_source: 'ai', triage_reason: 'ai:ollama' } });
    expect(historyText(ev, item, t, undefined)).toBe('Sorted into Read later by Ollama');
  });

  it('not set up text', () => {
    expect(whyText({ ...item, triageSource: 'none', triageReason: 'none:not-set-up' }, t, '1 GB')).toEqual({
      text: 'Sorting with rules until the AI provider is set up in Integrations.',
    });
  });
});
