// Settings › Triage model cards: one card per registry model, live from `ai-status`.
import { useQueryClient } from '@tanstack/react-query';
import { Button, StatusPill } from '../../ds';
import { errorMessage, errorText } from '../../shared/errors';
import { formatSize } from '../../shared/format';
import { useLang, useT, type Lang, type TFunction } from '../../shared/i18n';
import { aiDownload, aiPause, aiSelectModel } from '../../shared/ipc';
import { AI_STATUS_KEY, SETTINGS_KEY, useAiStatus, useSetting } from '../../shared/queries';
import { useToast } from '../../shared/Toast';
import type { AiStatus, ModelInfo } from '../../shared/types';
import { ProgressBar } from './parts';

/** The registry model whose label carries a memory requirement. */
const NEEDS_16GB = 'qwen3-4b-instruct-2507-q4km';

/** Size in the UI language, plus the memory note where the model has one. */
export function modelMeta(m: ModelInfo, t: TFunction, lang: Lang): string {
  const size = formatSize(m.bytes, lang);
  return m.id === NEEDS_16GB ? `${size} · ${t('settings.triage.needs16')}` : size;
}

/** "Qwen3 4B (2.5 GB), min RAM 16 GB" → "Qwen3 4B"; the size is formatted per language instead. */
export function modelName(m: ModelInfo): string {
  const cut = m.label.indexOf(' (');
  return cut > 0 ? m.label.slice(0, cut) : m.label;
}

/** The download for `id` is in flight, paused, verifying or failed. */
export function transferOf(s: AiStatus, id: string): AiStatus['state'] | null {
  if (s.modelId !== id) return null;
  return s.state === 'downloading' || s.state === 'paused' || s.state === 'verifying' || s.state === 'failed'
    ? s.state
    : null;
}

export function downloadPercent(s: AiStatus): number {
  return s.bytesTotal > 0 ? Math.floor((s.bytesDone * 100) / s.bytesTotal) : 0;
}

export function aiErrorReason(t: TFunction, error: string | null): string {
  if (error === 'checksum') return t('settings.triage.errorChecksum');
  if (error === 'localOnly') return t('settings.triage.localOnlyBlocked');
  return error ?? '';
}

/** Progress line + strip + Pause/Resume/Try again for an in-flight download. */
export function TransferStatus({ status }: { status: AiStatus }) {
  const t = useT();
  const lang = useLang();
  const run = useAiAction();
  const pct = downloadPercent(status);
  const id = status.modelId ?? '';
  let line: string;
  let action: { label: string; icon?: 'pause' | 'play'; fn: () => Promise<void> } | null = null;
  switch (status.state) {
    case 'downloading':
      line = t('settings.triage.progress', {
        done: formatSize(status.bytesDone, lang),
        total: formatSize(status.bytesTotal, lang),
        percent: pct,
      });
      action = { label: t('common.pause'), icon: 'pause', fn: aiPause };
      break;
    case 'paused':
      line = t('settings.triage.pausedAt', { percent: pct });
      action = { label: t('common.resume'), icon: 'play', fn: () => aiDownload(id) };
      break;
    case 'verifying':
      line = t('settings.triage.verifying');
      break;
    default:
      line = t('settings.triage.failed', { reason: aiErrorReason(t, status.error) });
      action = { label: t('common.retry'), fn: () => aiDownload(id) };
  }
  const act = action;
  return (
    <div className="st-model__status" role="status">
      <div className="st-model__line">
        <span className={status.state === 'failed' ? 'st-error' : undefined}>{line}</span>
        {act ? (
          <Button variant="quiet" size="sm" icon={act.icon} onClick={() => run(act.fn)}>
            {act.label}
          </Button>
        ) : null}
      </div>
      {status.state === 'failed' ? null : (
        <ProgressBar
          value={status.state === 'verifying' ? null : pct / 100}
          label={t('settings.triage.progressLabel', { percent: pct })}
        />
      )}
    </div>
  );
}

/** Runs an AI command, refreshes status + settings, and toasts a failure. */
export function useAiAction(): (fn: () => Promise<void>) => Promise<void> {
  const qc = useQueryClient();
  const toast = useToast();
  const t = useT();
  return async (fn) => {
    try {
      await fn();
    } catch (e) {
      const code = errorText(e);
      toast.show({ message: code === 'localOnly' ? t('settings.triage.localOnlyBlocked') : errorMessage(t, e) });
    } finally {
      void qc.invalidateQueries({ queryKey: AI_STATUS_KEY });
      void qc.invalidateQueries({ queryKey: SETTINGS_KEY });
    }
  };
}

export function ModelCards() {
  const t = useT();
  const lang = useLang();
  const run = useAiAction();
  const { data: status } = useAiStatus();
  const activeId = useSetting<string | null>('ai.modelId', null);
  const localOnly = useSetting<boolean>('privacy.localOnly', false);
  if (!status) return null;

  return (
    <div className="st-models">
      {status.models.map((m) => {
        const transfer = transferOf(status, m.id);
        const active = activeId === m.id && m.downloaded;
        return (
          <article key={m.id} className={active ? 'st-model is-active' : 'st-model'} aria-label={modelName(m)}>
            <div className="st-model__head">
              <div className="st-model__title">
                <span className="st-model__name">{modelName(m)}</span>
                <span className="st-model__meta">{modelMeta(m, t, lang)}</span>
              </div>
              {m.id === status.recommendedId ? (
                <StatusPill status="local" icon="idea" label={t('settings.triage.recommended')} />
              ) : null}
              <div className="st-model__actions">
                {active ? (
                  <StatusPill status="synced" label={t('settings.triage.inUse')} />
                ) : m.downloaded ? (
                  <>
                    <StatusPill status="local" icon="check" label={t('settings.triage.downloaded')} />
                    <Button size="sm" onClick={() => run(() => aiSelectModel(m.id))}>
                      {t('settings.triage.useModel')}
                    </Button>
                  </>
                ) : transfer ? null : (
                  <Button size="sm" disabled={localOnly} onClick={() => run(() => aiDownload(m.id))}>
                    {t('common.download')}
                  </Button>
                )}
              </div>
            </div>
            {transfer ? <TransferStatus status={status} /> : null}
          </article>
        );
      })}
    </div>
  );
}
