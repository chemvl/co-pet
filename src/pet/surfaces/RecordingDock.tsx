// The Recording HUD beside the pet while a meeting records: timer, Me/System meters, the
// meeting language, Pause/Resume and Stop. Levels arrive on `recording-level` (pet window only).
import { useEffect, useState } from 'react';
import { RecordingHUD } from '../../ds';
import { formatClock } from '../../shared/format';
import { useT, type TFunction } from '../../shared/i18n';
import { on } from '../../shared/ipc';
import type { MeetingLanguage, RecordingStatus } from '../../shared/types';

/** The HUD's short language label: `Auto`, `en`, `vi`, `zh`, `vi + en`. */
export function shortLanguage(lang: MeetingLanguage, t: TFunction): string {
  if (lang === 'auto') return t('rec.lang.auto');
  if (lang === 'vi+en') return 'vi + en';
  return lang;
}

export function RecordingDock(p: {
  status: RecordingStatus;
  /** when `status` arrived: the clock runs on from `status.elapsedMs` while recording */
  receivedAt: number;
  onPause(): void;
  onStop(): void;
  onLanguage(anchor: HTMLButtonElement): void;
}) {
  const t = useT();
  const recording = p.status.state === 'recording';
  const [levels, setLevels] = useState({ mic: 0, system: 0 });
  const [, tick] = useState(0);

  useEffect(() => {
    const off = on('recording-level', setLevels);
    return () => void off.then((f) => f());
  }, []);

  // Re-render every second while the clock runs; the time itself is read below.
  useEffect(() => {
    if (!recording) return;
    const timer = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(timer);
  }, [recording]);

  const elapsed = p.status.elapsedMs + (recording ? Math.max(0, Date.now() - p.receivedAt) : 0);
  return (
    <RecordingHUD
      elapsed={formatClock(elapsed)}
      micLevel={recording ? levels.mic : 0}
      systemLevel={recording ? levels.system : 0}
      language={shortLanguage(p.status.language, t)}
      paused={!recording}
      labels={{
        me: t('rec.hud.me'),
        system: t('rec.hud.system'),
        pause: t('rec.hud.pause'),
        resume: t('rec.hud.resume'),
        stop: t('rec.hud.stop'),
        recording: t('rec.hud.recording'),
        paused: t('rec.hud.paused'),
        language: t('rec.hud.language'),
      }}
      onPause={p.onPause}
      onStop={p.onStop}
      onLanguage={p.onLanguage}
    />
  );
}
