// Display text derived from items: metadata lines, "Why it's here" and history entries.
import { categoryLabelOf } from '../shared/categories';
import type { Lang, TFunction } from '../shared/i18n';
import { formatDate, formatSize, formatTime, joinMeta } from '../shared/format';
import type { Category, ItemDetail, ItemEvent, ItemKind, ItemSummary, Json, SourceKind } from '../shared/types';

const FILE_KINDS: readonly ItemKind[] = ['file', 'image', 'screenshot'];

/** URL host without `www.`; null for missing or unparsable URLs. */
export function hostOf(url: string | null): string | null {
  if (!url) return null;
  try {
    return new URL(url).hostname.replace(/^www\./, '') || null;
  } catch {
    return null;
  }
}

/** Lowercase extension of a file name, without the dot. */
export function extOf(name: string | null): string | null {
  const m = name ? /\.([^./\\]+)$/.exec(name) : null;
  return m ? m[1].toLowerCase() : null;
}

/** "Quick note", "Clipboard"…; drops have no label of their own (the source app says it). */
function sourceKindLabel(kind: SourceKind, t: TFunction): string | null {
  return kind === 'drop' ? null : t(`home.source.${kind}`);
}

/** Today → `14:02`; earlier → `Fri 17 Oct` (`T6, 17/10`), plus the time when `withTime`. */
export function whenShort(ms: number, lang: Lang, withTime = false): string {
  const d = new Date(ms);
  if (d.toDateString() === new Date().toDateString()) return formatTime(d);
  return withTime ? `${formatDate(d, lang)} ${formatTime(d)}` : formatDate(d, lang);
}

/** Row metadata: source · context · time (`developers.notion.com · Chrome · 14:02`). */
export function rowMeta(item: ItemSummary, t: TFunction, lang: Lang): string {
  return joinMeta([
    item.kind === 'url' ? hostOf(item.url) : null,
    sourceKindLabel(item.sourceKind, t),
    item.sourceApp,
    FILE_KINDS.includes(item.kind) && item.sizeBytes != null ? formatSize(item.sizeBytes, lang) : null,
    whenShort(item.capturedAt, lang),
  ]);
}

/** Detail header metadata: source app · URL host · date and time. */
export function detailMeta(item: ItemSummary, t: TFunction, lang: Lang): string {
  const d = new Date(item.capturedAt);
  return joinMeta([
    item.sourceApp ?? sourceKindLabel(item.sourceKind, t),
    hostOf(item.url),
    FILE_KINDS.includes(item.kind) && item.sizeBytes != null ? formatSize(item.sizeBytes, lang) : null,
    `${formatDate(d, lang)} ${formatTime(d)}`,
  ]);
}

/** What "Make it a rule" would create for this item, if anything (url → domain, file → extension). */
export function ruleTargetOf(item: ItemSummary): { kind: 'domain' | 'extension'; value: string } | null {
  if (item.kind === 'url') {
    const host = hostOf(item.url);
    return host ? { kind: 'domain', value: host } : null;
  }
  if (FILE_KINDS.includes(item.kind)) {
    const ext = extOf(item.fileName);
    return ext ? { kind: 'extension', value: ext } : null;
  }
  return null;
}

export interface WhyText {
  text: string;
  /** 0..1, for the AI confidence bar */
  confidence?: number;
}

/** Renders the language-neutral `triage_reason` code. `modelSize` fills `none:no-model`. */
export function whyText(item: ItemSummary, t: TFunction, modelSize: string): WhyText | null {
  if (item.triageStatus === 'pending') return { text: t('common.sorting') };
  const reason = item.triageReason;
  if (!reason) return null;
  if (reason === 'user') return { text: t('item.why.user') };
  if (reason.startsWith('rule:')) {
    const [, kind, ...rest] = reason.split(':');
    const value = rest.join(':');
    switch (kind) {
      case 'domain':
        return { text: t('item.why.domain', { value }) };
      case 'extension':
        return { text: t('item.why.extension', { value }) };
      case 'kind':
        return { text: t('item.why.kind', { value: isKind(value) ? t(`kind.${value}`) : value }) };
      case 'source':
        return { text: t('item.why.source', { value: isSourceKind(value) ? t(`home.source.${value}`) : value }) };
      case 'regex':
        return { text: value === 'contact' ? t('item.why.regexContact') : t('item.why.rule') };
      default:
        return { text: t('item.why.rule') };
    }
  }
  if (reason === 'ai' || item.triageSource === 'ai') {
    return {
      text: item.categoryId === 'unsorted' ? t('item.why.aiUnsure') : t('item.why.ai'),
      confidence: item.triageConfidence ?? undefined,
    };
  }
  switch (reason) {
    case 'none:no-model':
      return { text: t('ai.rulesUntilDownload', { size: modelSize }) };
    case 'none:sensitive':
      return { text: t('item.why.sensitive') };
    case 'none:ai-off':
      return { text: t('item.why.aiOff') };
    case 'none:ai-error':
      return { text: t('item.why.aiError') };
  }
  return null;
}

function isKind(s: string): s is ItemKind {
  return ['file', 'image', 'url', 'text', 'richtext', 'code', 'screenshot'].includes(s);
}

function isSourceKind(s: string): s is SourceKind {
  return ['drop', 'clipboard', 'screenshot', 'note', 'selection'].includes(s);
}

function jsonField(j: Json | null, key: string): string | null {
  if (j && typeof j === 'object' && !Array.isArray(j)) {
    const v = j[key];
    return typeof v === 'string' ? v : null;
  }
  return null;
}

/** One history line, e.g. "Sorted into Reference by the local model". A deleted category shows its id. */
export function historyText(ev: ItemEvent, item: ItemDetail, t: TFunction, categories: readonly Category[] | undefined): string {
  switch (ev.action) {
    case 'captured': {
      const source = item.sourceApp ?? sourceKindLabel(item.sourceKind, t);
      return source ? t('item.history.capturedFrom', { source }) : t('item.history.captured');
    }
    case 'recategorize': {
      const cat = jsonField(ev.afterJson, 'category_id');
      const category = cat ? categoryLabelOf(categories, cat, t) : '';
      if (ev.actor === 'user') return t('item.history.moved', { category });
      const src = jsonField(ev.afterJson, 'triage_source');
      if (src === 'rule') return t('item.history.sortedRule', { category });
      if (src === 'ai') return t('item.history.sortedAi', { category });
      return t('item.history.sorted', { category });
    }
    default:
      return t(`item.history.${ev.action}`);
  }
}
