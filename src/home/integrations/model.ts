// Pure helpers for the export UI: provider names, task export pills, queue lines and the
// Notion mapping choices. No React, no IPC.
import type { Status } from '../../ds/types';
import type { TFunction } from '../../shared/i18n';
import type {
  ExportProvider, NotionMapping, NotionProp, NotionSchema, NotionStatusMap, ProviderStatus, TaskExportState,
} from '../../shared/types';

export const PROVIDERS: readonly ExportProvider[] = ['notion', 'asana'];

export const PROVIDER_NAME: Readonly<Record<ExportProvider, string>> = { notion: 'Notion', asana: 'Asana' };

/** Connected, not waiting for a reconnect, with a target: tasks can be exported. */
export function isReady(s: ProviderStatus | undefined): boolean {
  return !!s && s.connected && !s.needsReconnect && s.target != null;
}

/** Whole seconds until `nextAttemptAt` (rounded up); 0 once due or without a time. */
export function retryInSeconds(nextAttemptAt: number | null, now: number): number {
  return nextAttemptAt == null ? 0 : Math.max(0, Math.ceil((nextAttemptAt - now) / 1000));
}

/** A queued job waiting out a rate limit or a network failure. */
export function isRetrying(e: TaskExportState): boolean {
  return e.state === 'queued' && (e.error === 'rateLimited' || e.error === 'network') && e.nextAttemptAt != null;
}

/**
 * The task row pill. A retry that is already due (or paused by local-only mode) reads as plain
 * "Queued" rather than "retrying in 0 s".
 */
export function exportPill(e: TaskExportState, now: number, t: TFunction): { status: Status; label: string } {
  const provider = PROVIDER_NAME[e.provider];
  switch (e.state) {
    case 'queued': {
      const s = isRetrying(e) ? retryInSeconds(e.nextAttemptAt, now) : 0;
      return s > 0
        ? { status: 'due', label: t('tasks.export.retrying', { provider, s }) }
        : { status: 'pending', label: t('tasks.export.queued') };
    }
    case 'exporting':
      return { status: 'exporting', label: t('tasks.export.exporting') };
    case 'exported':
      return { status: 'exported', label: provider };
    case 'failed':
      return { status: 'failed', label: t('tasks.export.failed') };
  }
}

/** Why this device's jobs of `s` wait: the last job error. Null when nothing went wrong. */
export function queueReason(s: ProviderStatus, t: TFunction): string | null {
  const provider = PROVIDER_NAME[s.provider];
  switch (s.lastError) {
    case 'rateLimited':
      return t('integrations.reason.rateLimited', { provider });
    case 'network':
      return t('integrations.reason.network', { provider });
    case 'unauthorized':
    case 'disconnected':
      return t('integrations.reason.reconnect');
    case 'targetMissing':
      return t('integrations.reason.targetMissing');
    case 'invalid':
      return t('integrations.reason.failed');
    case null:
      return s.needsReconnect ? t('integrations.reason.reconnect') : null;
  }
}

/** "{n} export waiting · {reason}" ("{n} export waiting" without one), or null when nothing waits. */
export function queueLine(s: ProviderStatus, t: TFunction): string | null {
  const n = s.queued + s.failed;
  if (n <= 0) return null;
  const reason = queueReason(s, t);
  if (reason == null) return n === 1 ? t('integrations.queueOneBare') : t('integrations.queueBare', { n });
  return n === 1 ? t('integrations.queueOne', { reason }) : t('integrations.queue', { n, reason });
}

// ---------- Notion mapping ----------

/** The schema's properties a mapping row may pick, in column order. */
export function propsOf(schema: NotionSchema, kinds: readonly string[]): NotionProp[] {
  return schema.props.filter((p) => kinds.includes(p.kind));
}

/**
 * The status mapping for property `name`: the schema's suggestion when it is that property,
 * else its first option for open tasks and its last for done ones. Null for "Not mapped" or a
 * property without options.
 */
export function statusMapFor(schema: NotionSchema, name: string | null): NotionStatusMap | null {
  if (!name) return null;
  if (schema.suggested.status?.prop === name) return schema.suggested.status;
  const prop = schema.props.find((p) => p.name === name);
  if (!prop || (prop.kind !== 'status' && prop.kind !== 'select') || !prop.options.length) return null;
  return { prop: name, kind: prop.kind, todo: prop.options[0], done: prop.options[prop.options.length - 1] };
}

/** `mapping` with names the schema no longer has dropped (title falls back to the suggestion). */
export function reconcileMapping(schema: NotionSchema, mapping: NotionMapping): NotionMapping {
  const has = (name: string | null | undefined, kinds: readonly string[]) =>
    !!name && schema.props.some((p) => p.name === name && kinds.includes(p.kind));
  const status = mapping.status && has(mapping.status.prop, ['status', 'select']) ? mapping.status : null;
  return {
    title: has(mapping.title, ['title']) ? mapping.title : schema.suggested.title,
    status,
    due: has(mapping.due, ['date']) ? mapping.due : null,
    tags: has(mapping.tags, ['multi_select']) ? mapping.tags : null,
    source: has(mapping.source, ['url']) ? mapping.source : null,
  };
}
