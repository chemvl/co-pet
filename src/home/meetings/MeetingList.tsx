// The Meetings list pane: Record meeting (or the live recording with Stop), Import audio or
// video, one row per meeting with its date, length and languages or its processing state, and
// the audio retention note.
import { useEffect, useState, type ReactElement } from 'react';
import { Button, StatusPill } from '../../ds';
import { formatClock, formatDate, formatDuration, joinMeta } from '../../shared/format';
import { useLang, useT, type Lang, type TFunction } from '../../shared/i18n';
import { useRecording, useSetting } from '../../shared/queries';
import type { MeetingSummary } from '../../shared/types';
import { useMeetingActions } from './recordFlow';

const TICK_MS = 1000;

/** What the meeting is waiting on, or null when it reads as a finished meeting. */
function stateText(m: MeetingSummary, t: TFunction): string | null {
  switch (m.state) {
    case 'recording':
      return m.onThisDevice ? t('rec.hud.recording') : t('meetings.state.recordingElsewhere');
    case 'queued':
      if (!m.onThisDevice) return t('meetings.state.elsewhere');
      return t(m.error === 'needsSpeechModel' ? 'meetings.state.needsModel' : 'meetings.state.queued');
    case 'transcribing':
      if (!m.onThisDevice) return t('meetings.state.elsewhere');
      return t('meetings.state.transcribing', { percent: Math.floor((m.progress ?? 0) * 100) });
    case 'failed':
      return null;
    case 'done':
      return m.summaryState === 'pending' || m.summaryState === 'running' ? t('meetings.state.summarizing') : null;
  }
}

/** `Thu 15 Oct · 42 min · vi + en`; while processing `Imported · 3 min · Transcribing 40%`. */
function rowMeta(m: MeetingSummary, t: TFunction, lang: Lang): string {
  const state = stateText(m, t);
  const imported = m.source === 'import' && t('meetings.imported');
  const duration = m.durationMs > 0 && formatDuration(m.durationMs, lang);
  if (state || m.state === 'failed') return joinMeta([imported, duration, state]);
  return joinMeta([imported, formatDate(m.startedAt, lang), duration, m.langs.join(' + ')]);
}

export function MeetingList(p: { meetings: MeetingSummary[]; selectedId: string | undefined; full: boolean }): ReactElement {
  const t = useT();
  const lang = useLang();
  const actions = useMeetingActions();
  const retention = useSetting<number | null>('transcribe.audioRetentionDays', 30);
  const { data: rec, dataUpdatedAt } = useRecording();
  const live = rec?.state === 'recording';

  // The elapsed time counts up between `recording-status` events.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!live) return;
    const id = window.setInterval(() => setNow(Date.now()), TICK_MS);
    return () => window.clearInterval(id);
  }, [live]);
  const elapsed = rec ? rec.elapsedMs + (live ? Math.max(0, now - dataUpdatedAt) : 0) : 0;

  return (
    <nav className={`mt-list${p.full ? ' mt-list--full' : ''}`} aria-label={t('meetings.title')}>
      <div className="mt-list__actions">
        {actions.recording ? (
          <div className="mt-list__rec">
            <StatusPill
              status="failed"
              className="mt-rec"
              label={live ? t('meetings.recording', { time: formatClock(elapsed) }) : joinMeta([t('rec.hud.paused'), formatClock(elapsed)])}
            />
            <Button variant="danger" size="sm" icon="stop" disabled={actions.busy} onClick={actions.stop}>
              {t('meetings.stop')}
            </Button>
          </div>
        ) : (
          <Button variant="primary" icon="mic" disabled={actions.busy} onClick={actions.record}>
            {t('meetings.record')}
          </Button>
        )}
        <Button variant="quiet" size="sm" onClick={actions.importFile}>
          {t('meetings.import')}
        </Button>
      </div>
      {p.meetings.map((m) => (
        <a
          key={m.id}
          href={`#/meetings/${encodeURIComponent(m.id)}`}
          className={`mt-row${m.id === p.selectedId ? ' is-sel' : ''}`}
          aria-current={m.id === p.selectedId ? 'page' : undefined}
        >
          <span className="body-strong hm-ellipsis mt-row__t">{m.title}</span>
          <span className="caption hm-muted hm-ellipsis">{rowMeta(m, t, lang)}</span>
          {m.state === 'failed' ? <StatusPill status="failed" label={t('meetings.state.failed')} /> : null}
        </a>
      ))}
      <div className="mt-list__spacer" />
      <span className="caption hm-muted mt-list__foot">
        {retention == null ? t('meetings.retention.keep') : t('meetings.retention.days', { n: retention })}
      </span>
    </nav>
  );
}
