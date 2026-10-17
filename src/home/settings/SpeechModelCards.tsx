// Speech-to-text and speaker-separation model packs, live from `speech-status`.
import { useQueryClient } from '@tanstack/react-query';
import { Button, StatusPill } from '../../ds';
import { errorMessage } from '../../shared/errors';
import { formatSize } from '../../shared/format';
import { useLang, useT, type I18nKey } from '../../shared/i18n';
import { speechDownload, speechPause } from '../../shared/ipc';
import { SETTINGS_KEY, SPEECH_STATUS_KEY, useSetting, useSpeechStatus } from '../../shared/queries';
import { useToast } from '../../shared/Toast';
import type { PackStatus, SpeechPackId } from '../../shared/types';
import { aiErrorReason } from './ModelCards';
import { ProgressBar } from './parts';

const PACK_COPY: Record<SpeechPackId, { name: I18nKey; desc: I18nKey }> = {
  speech: { name: 'speech.stt', desc: 'speech.stt.desc' },
  speaker: { name: 'speech.speaker', desc: 'speech.speaker.desc' },
};

/** Runs a speech command, refreshes status + settings, and toasts a failure. */
function useSpeechAction(): (fn: () => Promise<void>) => Promise<void> {
  const qc = useQueryClient();
  const toast = useToast();
  const t = useT();
  return async (fn) => {
    try {
      await fn();
    } catch (e) {
      toast.show({ message: errorMessage(t, e) });
    } finally {
      void qc.invalidateQueries({ queryKey: SPEECH_STATUS_KEY });
      void qc.invalidateQueries({ queryKey: SETTINGS_KEY });
    }
  };
}

/** Progress line + strip + Pause/Resume/Try again for an in-flight pack download. */
function PackTransfer({ pack }: { pack: PackStatus }) {
  const t = useT();
  const lang = useLang();
  const run = useSpeechAction();
  const pct = pack.bytesTotal > 0 ? Math.floor((pack.bytesDone * 100) / pack.bytesTotal) : 0;
  let line: string;
  let action: { label: string; icon?: 'pause' | 'play'; fn: () => Promise<void> } | null = null;
  switch (pack.state) {
    case 'downloading':
      line = t('settings.triage.progress', {
        done: formatSize(pack.bytesDone, lang),
        total: formatSize(pack.bytesTotal, lang),
        percent: pct,
      });
      action = { label: t('common.pause'), icon: 'pause', fn: () => speechPause(pack.id) };
      break;
    case 'paused':
      line = t('settings.triage.pausedAt', { percent: pct });
      action = { label: t('common.resume'), icon: 'play', fn: () => speechDownload(pack.id) };
      break;
    case 'verifying':
      line = t('settings.triage.verifying');
      break;
    default:
      line = t('settings.triage.failed', { reason: aiErrorReason(t, pack.error) });
      action = { label: t('common.retry'), fn: () => speechDownload(pack.id) };
  }
  const act = action;
  return (
    <div className="st-model__status" role="status">
      <div className="st-model__line">
        <span className={pack.state === 'failed' ? 'st-error' : undefined}>{line}</span>
        {act ? (
          <Button variant="quiet" size="sm" icon={act.icon} onClick={() => run(act.fn)}>
            {act.label}
          </Button>
        ) : null}
      </div>
      {pack.state === 'failed' ? null : (
        <ProgressBar
          value={pack.state === 'verifying' ? null : pct / 100}
          label={t('settings.triage.progressLabel', { percent: pct })}
        />
      )}
    </div>
  );
}

export function SpeechModelCards() {
  const t = useT();
  const lang = useLang();
  const run = useSpeechAction();
  const { data: status } = useSpeechStatus();
  const localOnly = useSetting<boolean>('privacy.localOnly', false);
  if (!status) return null;
  const blocked = localOnly && status.packs.some((p) => p.state === 'none');

  return (
    <div className="st-models">
      {status.packs.map((p) => {
        const copy = PACK_COPY[p.id];
        const name = t(copy.name);
        const inFlight = p.state === 'downloading' || p.state === 'paused' || p.state === 'verifying' || p.state === 'failed';
        const meta = p.bytesTotal > 0 ? `${t(copy.desc)} · ${formatSize(p.bytesTotal, lang)}` : t(copy.desc);
        return (
          <article key={p.id} className="st-model" aria-label={name}>
            <div className="st-model__head">
              <div className="st-model__title">
                <span className="st-model__name">{name}</span>
                <span className="st-model__meta">{meta}</span>
              </div>
              <div className="st-model__actions">
                {p.state === 'ready' ? (
                  <StatusPill status="local" icon="check" label={t('settings.triage.downloaded')} />
                ) : inFlight ? null : (
                  <Button size="sm" disabled={localOnly} onClick={() => run(() => speechDownload(p.id))}>
                    {t('common.download')}
                  </Button>
                )}
              </div>
            </div>
            {inFlight ? <PackTransfer pack={p} /> : null}
          </article>
        );
      })}
      {blocked ? <p className="st-body">{t('settings.triage.localOnlyBlocked')}</p> : null}
    </div>
  );
}
