// Agents screen text: the Activity table's "What" column and action cell, and the client
// cards' relative "last call". Pure, so both languages are tested.
import { categoryLabelOf } from '../../shared/categories';
import { errorMessage } from '../../shared/errors';
import { formatDate } from '../../shared/format';
import type { Lang, TFunction } from '../../shared/i18n';
import type { ActivityRow, Category, ExportProvider } from '../../shared/types';
import { PROVIDER_NAME } from '../integrations/model';

const MIN_MS = 60_000;
const HOUR_MS = 60 * MIN_MS;
const DAY_MS = 24 * HOUR_MS;

function str(v: unknown): string | null {
  return typeof v === 'string' ? v : null;
}

function isProvider(v: unknown): v is ExportProvider {
  return v === 'notion' || v === 'asana';
}

/** `detail.count` as "1 result" / "{n} results". */
function results(count: unknown, t: TFunction): string {
  const n = typeof count === 'number' ? count : 0;
  return n === 1 ? t('agents.result') : t('agents.results', { n });
}

/** The "What" of one logged call, from its tool and `detail`. */
export function activityText(row: ActivityRow, t: TFunction, categories?: readonly Category[]): string {
  if (row.error === 'writesNotApproved') return t('agents.act.blockedWrites');
  if (row.error === 'paused') return t('agents.act.blockedPaused');
  if (row.error) return t('agents.act.failed', { reason: errorMessage(t, row.error) });
  const d = row.detail;
  const title = str(d.title) ?? '';
  switch (row.tool) {
    case 'search_items': {
      const query = str(d.query);
      const count = results(d.count, t);
      return query ? t('agents.act.search', { query, results: count }) : t('agents.act.list', { results: count });
    }
    case 'get_item':
      return t('agents.act.read', { title });
    case 'list_tasks':
      return t('agents.act.tasks', { results: results(d.count, t) });
    case 'whats_next':
      return title ? t('agents.act.next', { title }) : t('agents.act.nextNone');
    case 'add_item':
      return t('agents.act.add', { title });
    case 'update_item': {
      const id = str(d.category);
      if (id) {
        // A live category in today's language; a deleted one by the name it had then.
        const live = categories?.some((c) => c.id === id);
        const category = (!live && str(d.categoryName)) || categoryLabelOf(categories, id, t);
        return t('agents.act.move', { title, category });
      }
      if (d.archived === true) return t('agents.act.archive', { title });
      if (d.archived === false) return t('agents.act.unarchive', { title });
      if (Array.isArray(d.tags)) return t('agents.act.tag', { title });
      return t('agents.act.update', { title });
    }
    case 'create_task':
      return t('agents.act.createTask', { title });
    case 'update_task':
      return t('agents.act.updateTask', { title });
    case 'complete_task':
      return t('agents.act.complete', { title });
    case 'export_task':
      return t('agents.act.export', { title, provider: isProvider(d.provider) ? PROVIDER_NAME[d.provider] : '' });
    default:
      return t('agents.act.other', { tool: row.tool });
  }
}

export type ActivityAction =
  | { kind: 'undo' }
  | { kind: 'undone' }
  | { kind: 'read' }
  | { kind: 'open'; url: string; provider: ExportProvider }
  | null;

/** The last cell: Undo / Undone for undoable writes, "Read" for reads, "Open in …" for exports. */
export function activityAction(row: ActivityRow): ActivityAction {
  if (row.tool === 'export_task' && row.resultUrl) {
    const p = row.detail.provider;
    return isProvider(p) ? { kind: 'open', url: row.resultUrl, provider: p } : null;
  }
  if (row.undoable) return row.undoneAt != null ? { kind: 'undone' } : { kind: 'undo' };
  return row.isWrite ? null : { kind: 'read' };
}

/** "just now", "5 min ago", "3 h ago", else the date. */
export function relativeTime(ms: number, now: number, t: TFunction, lang: Lang): string {
  const ago = Math.max(0, now - ms);
  if (ago < MIN_MS) return t('agents.justNow');
  if (ago < HOUR_MS) return t('agents.minAgo', { n: Math.floor(ago / MIN_MS) });
  if (ago < DAY_MS) return t('agents.hoursAgo', { n: Math.floor(ago / HOUR_MS) });
  return formatDate(ms, lang);
}
