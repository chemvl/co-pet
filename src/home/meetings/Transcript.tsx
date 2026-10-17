// The Meetings transcript pane: title and facts, search, the More menu (rename, transcribe or
// summarize again, open the item, delete), the player, and the transcript lines or the state the
// meeting is in (recording, waiting for the speech model, transcribing, failed, no speech).
import { useEffect, useId, useMemo, useRef, useState, type ReactElement, type ReactNode } from 'react';
import { Button, SearchField, TranscriptSegment } from '../../ds';
import { errorMessage } from '../../shared/errors';
import { formatClock, formatDate, formatDuration, formatSize, formatTime, joinMeta } from '../../shared/format';
import { useLang, useT, type I18nKey } from '../../shared/i18n';
import {
  deleteMeeting, renameMeeting, renameSpeaker, resummarizeMeeting, retranscribeMeeting, retryMeeting, speechDownload,
  summarizeMeeting,
} from '../../shared/ipc';
import { useSetting, useSpeechStatus } from '../../shared/queries';
import { useToast } from '../../shared/Toast';
import type { MeetingDetail, MeetingLanguage, TaskChange } from '../../shared/types';
import { MenuButton, type MenuItem } from '../Menu';
import { matchRanges } from '../palette/match';
import { Highlight } from '../palette/Palette';
import { navigate } from '../router';
import { Dialog, ProgressBar } from '../settings/parts';
import { Player, usePlayer } from './Player';
import { activeIndex, changeBySegment, formatDay, speakerIndex, speakerName } from './segments';
import { LANG_NAME, summaryLangOf } from './SummaryPane';

const LANGUAGES: { value: MeetingLanguage; label: I18nKey }[] = [
  { value: 'auto', label: 'lang.auto' },
  { value: 'en', label: 'lang.en' },
  { value: 'vi', label: 'lang.vi' },
  { value: 'zh', label: 'lang.zh' },
  { value: 'vi+en', label: 'lang.viEn' },
];

type Open = null | { kind: 'rename' } | { kind: 'retranscribe' } | { kind: 'delete' } | { kind: 'speaker'; key: string };

export function Transcript({ meeting: m, onBack }: { meeting: MeetingDetail; onBack?: () => void }): ReactElement {
  const t = useT();
  const lang = useLang();
  const { show } = useToast();
  const fail = (e: unknown) => show({ message: errorMessage(t, e) });
  const player = usePlayer(m.durationMs, fail);
  const [searching, setSearching] = useState(false);
  const [query, setQuery] = useState('');
  const [open, setOpen] = useState<Open>(null);
  const segsRef = useRef<HTMLDivElement>(null);

  const playable = m.audio === 'available' && m.audioPath != null;
  const active = playable && player.playing ? activeIndex(m.transcript, player.atMs) : -1;
  const changes = useMemo(() => changeBySegment(m.transcript, m.changes), [m.transcript, m.changes]);

  useEffect(() => {
    if (active >= 0) segsRef.current?.querySelector('.cp-seg-line.is-active')?.scrollIntoView({ block: 'nearest' });
  }, [active]);

  const speaker = (key: string) => m.speakers.find((s) => s.key === key) ?? { key, name: null };
  const changeText = (c: TaskChange) =>
    c.field === 'due'
      ? t('meetings.change.segDue', { to: c.to ? formatDay(c.to, lang) : t('meetings.change.noDate') })
      : c.field === 'title'
        ? t('meetings.change.segRename', { to: c.to ?? '' })
        : t('meetings.change.segDone');

  const summarizeAgain =
    m.summaryState === 'done'
      ? () => resummarizeMeeting(m.id, summaryLangOf(m))
      : m.summaryState === 'skipped' || m.summaryState === 'failed'
        ? () => summarizeMeeting(m.id)
        : null;
  const itemId = m.itemId;
  const menu: MenuItem[] = [
    { key: 'rename', label: t('meetings.rename'), onSelect: () => setOpen({ kind: 'rename' }) },
    {
      key: 'retranscribe',
      label: t('meetings.retranscribe'),
      disabled: !playable,
      hint: playable ? undefined : t(`meetings.audio.${m.audio === 'available' ? 'processing' : m.audio}`),
      onSelect: () => setOpen({ kind: 'retranscribe' }),
    },
    {
      key: 'summarize',
      label: t('meetings.summarizeAgain'),
      disabled: !summarizeAgain,
      onSelect: () => void summarizeAgain?.().catch(fail),
    },
    {
      key: 'inbox',
      label: t('meetings.openInInbox'),
      disabled: !itemId,
      onSelect: () => itemId && navigate(`item/${encodeURIComponent(itemId)}`),
    },
    { key: 'delete', label: t('meetings.delete'), danger: true, onSelect: () => setOpen({ kind: 'delete' }) },
  ];

  const caption = joinMeta([
    formatDate(m.startedAt, lang),
    formatTime(m.startedAt),
    m.durationMs > 0 && formatDuration(m.durationMs, lang),
    m.langs.map((l) => (LANG_NAME[l] ? t(LANG_NAME[l]) : l)).join(' + '),
    m.speakerCount > 0 && t(m.speakerCount === 1 ? 'meetings.speakers.one' : 'meetings.speakers.other', { n: m.speakerCount }),
  ]);

  const q = searching ? query.trim() : '';
  const rows = m.transcript.flatMap((seg, i) => {
    const ranges = q ? matchRanges(seg.text, q) : [];
    return ranges ? [{ seg, i, ranges }] : [];
  });

  let body: ReactNode;
  if (m.state === 'recording') {
    body = <Message text={t(m.onThisDevice ? 'rec.hud.recording' : 'meetings.state.recordingElsewhere')} />;
  } else if (!m.onThisDevice && (m.state === 'queued' || m.state === 'transcribing')) {
    body = <Message text={t('meetings.state.elsewhere')} />;
  } else if (m.state === 'queued') {
    body = m.error === 'needsSpeechModel' ? <NeedsSpeechModel /> : <Message text={t('meetings.state.queued')} />;
  } else if (m.state === 'transcribing') {
    const percent = Math.floor((m.progress ?? 0) * 100);
    body = (
      <div className="mt-msg">
        <ProgressBar value={m.progress} label={t('meetings.transcribing', { percent })} />
        <p className="body">{t('meetings.transcribing', { percent })}</p>
        <p className="caption hm-muted">{t('meetings.transcribingHint')}</p>
      </div>
    );
  } else if (m.state === 'failed') {
    body = (
      <div className="mt-msg" role="alert">
        <p className="body">{errorMessage(t, m.error ?? 'transcribeFailed')}</p>
        {m.onThisDevice ? (
          <Button variant="secondary" size="sm" onClick={() => void retryMeeting(m.id).catch(fail)}>
            {t('common.retry')}
          </Button>
        ) : null}
      </div>
    );
  } else if (!m.transcript.length) {
    body = <Message text={t('meetings.noSpeech')} />;
  } else if (!rows.length) {
    body = <Message text={t('meetings.noMatches')} />;
  } else {
    const multiLang = m.langs.length >= 2;
    body = (
      <div ref={segsRef} className="mt-segs">
        {rows.map(({ seg, i, ranges }) => {
          const sp = speaker(seg.speaker);
          const name = speakerName(sp, t);
          const start = formatClock(seg.startMs);
          const isMe = seg.speaker === 'me';
          return (
            <TranscriptSegment
              key={i}
              start={start}
              speaker={name}
              isMe={isMe}
              speakerIndex={speakerIndex(seg.speaker)}
              language={multiLang ? seg.lang : undefined}
              active={i === active}
              taskChange={changes.get(i)?.map(changeText).join(' · ')}
              onPlay={playable ? () => player.seek(seg.startMs, true) : undefined}
              playLabel={t('meetings.playFrom', { time: start })}
              onSpeaker={isMe ? undefined : () => setOpen({ kind: 'speaker', key: seg.speaker })}
              speakerLabel={isMe ? undefined : t('meetings.renameSpeakerOf', { name })}
            >
              {q ? <Highlight text={seg.text} ranges={ranges} /> : seg.text}
            </TranscriptSegment>
          );
        })}
      </div>
    );
  }

  const close = () => setOpen(null);
  let dialog: ReactElement | null = null;
  if (open?.kind === 'rename') {
    dialog = (
      <NameDialog
        title={t('meetings.rename')}
        label={t('meetings.name')}
        initial={m.title}
        required
        onSave={(name) => renameMeeting(m.id, name)}
        onClose={close}
      />
    );
  } else if (open?.kind === 'speaker') {
    const key = open.key;
    dialog = (
      <NameDialog
        title={t('meetings.renameSpeaker')}
        caption={t('meetings.renameSpeakerCaption')}
        label={t('meetings.speakerName')}
        initial={speaker(key).name ?? ''}
        onSave={(name) => renameSpeaker(m.id, key, name || null)}
        onClose={close}
      />
    );
  } else if (open?.kind === 'retranscribe') {
    dialog = <RetranscribeDialog meeting={m} onClose={close} />;
  } else if (open?.kind === 'delete') {
    dialog = <DeleteDialog meeting={m} onClose={close} />;
  }

  return (
    <section className="mt-tr" aria-label={t('meetings.transcript')}>
      <div className="mt-th">
        <div className="mt-th__row">
          {onBack ? (
            <Button variant="quiet" size="sm" icon="undo" onClick={onBack}>
              {t('common.back')}
            </Button>
          ) : null}
          <div className="mt-th__text">
            <h1 className="heading hm-ellipsis mt-th__title">{m.title}</h1>
            <div className="caption hm-muted hm-ellipsis">{caption}</div>
          </div>
          <Button
            variant="quiet"
            size="sm"
            icon="search"
            aria-label={t('meetings.search')}
            aria-pressed={searching}
            onClick={() => {
              setSearching((s) => !s);
              setQuery('');
            }}
          />
          <MenuButton variant="quiet" size="sm" icon="more" aria-label={t('item.moreActions')} menuLabel={t('item.moreActions')} align="end" items={menu} />
        </div>
        {searching ? (
          <SearchField
            autoFocus
            placeholder={t('meetings.search')}
            aria-label={t('meetings.search')}
            value={query}
            onChange={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                setSearching(false);
                setQuery('');
              }
            }}
          />
        ) : null}
        {playable && m.audioPath ? (
          <Player player={player} audioPath={m.audioPath} />
        ) : m.audio !== 'available' && m.state !== 'recording' ? (
          <span className="caption hm-muted">{t(`meetings.audio.${m.audio}`)}</span>
        ) : null}
      </div>
      {body}
      {dialog}
    </section>
  );
}

function Message({ text }: { text: string }): ReactElement {
  return (
    <div className="mt-msg">
      <p className="body hm-muted">{text}</p>
    </div>
  );
}

/** Queued without the speech pack: download both packs (speech, then speaker). */
function NeedsSpeechModel(): ReactElement {
  const t = useT();
  const lang = useLang();
  const { show } = useToast();
  const packs = useSpeechStatus().data?.packs ?? [];
  const localOnly = useSetting<boolean>('privacy.localOnly', false);
  const speech = packs.find((p) => p.id === 'speech');
  const inFlight = packs.some((p) => p.state === 'downloading' || p.state === 'verifying');
  const size = packs.reduce((sum, p) => sum + p.bytesTotal, 0);
  const download = async () => {
    try {
      for (const id of ['speech', 'speaker'] as const) {
        if (packs.find((p) => p.id === id)?.state !== 'ready') await speechDownload(id);
      }
    } catch (e) {
      show({ message: errorMessage(t, e) });
    }
  };
  return (
    <div className="mt-msg">
      <p className="body">{t('meetings.needsModel')}</p>
      {inFlight && speech ? (
        <ProgressBar
          value={speech.bytesTotal > 0 ? speech.bytesDone / speech.bytesTotal : null}
          label={t('meetings.downloadModels', { size: formatSize(size, lang) })}
        />
      ) : (
        <Button variant="primary" disabled={localOnly || !packs.length} onClick={() => void download()}>
          {t('meetings.downloadModels', { size: formatSize(size, lang) })}
        </Button>
      )}
    </div>
  );
}

/** A one-field rename; an empty name is refused when `required`, else saved as cleared. */
function NameDialog(p: {
  title: string;
  label: string;
  caption?: string;
  initial: string;
  required?: boolean;
  onSave: (name: string) => Promise<unknown>;
  onClose: () => void;
}): ReactElement {
  const t = useT();
  const id = useId();
  const [name, setName] = useState(p.initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const blocked = busy || (p.required && !name.trim());
  const save = async () => {
    if (blocked) return;
    setBusy(true);
    setError(null);
    try {
      await p.onSave(name.trim());
      p.onClose();
    } catch (e) {
      setError(errorMessage(t, e));
      setBusy(false);
    }
  };
  return (
    <Dialog
      title={p.title}
      onClose={p.onClose}
      actions={
        <>
          <Button variant="quiet" onClick={p.onClose}>{t('common.cancel')}</Button>
          <Button variant="primary" disabled={blocked} onClick={() => void save()}>{t('common.save')}</Button>
        </>
      }
    >
      {p.caption ? <p className="body mt-m0">{p.caption}</p> : null}
      <label className="st-row__label" htmlFor={id}>{p.label}</label>
      <input
        id={id}
        className="st-input"
        value={name}
        autoFocus
        autoComplete="off"
        aria-invalid={error ? true : undefined}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => e.key === 'Enter' && void save()}
      />
      {error ? <p className="st-error" role="alert">{error}</p> : null}
    </Dialog>
  );
}

function RetranscribeDialog({ meeting: m, onClose }: { meeting: MeetingDetail; onClose: () => void }): ReactElement {
  const t = useT();
  const id = useId();
  const [language, setLanguage] = useState<MeetingLanguage>(m.language);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const start = async () => {
    setBusy(true);
    setError(null);
    try {
      await retranscribeMeeting(m.id, language);
      onClose();
    } catch (e) {
      setError(errorMessage(t, e));
      setBusy(false);
    }
  };
  return (
    <Dialog
      title={t('meetings.retranscribeTitle')}
      onClose={onClose}
      actions={
        <>
          <Button variant="quiet" onClick={onClose}>{t('common.cancel')}</Button>
          <Button variant="primary" disabled={busy} onClick={() => void start()}>{t('meetings.transcribe')}</Button>
        </>
      }
    >
      <label className="st-row__label" htmlFor={id}>{t('rec.hud.language')}</label>
      <select
        id={id}
        className="st-input"
        value={language}
        autoFocus
        onChange={(e) => setLanguage(e.currentTarget.value as MeetingLanguage)}
      >
        {LANGUAGES.map((l) => (
          <option key={l.value} value={l.value}>{t(l.label)}</option>
        ))}
      </select>
      <p className="caption hm-muted mt-m0">{t('meetings.retranscribeCaption')}</p>
      {error ? <p className="st-error" role="alert">{error}</p> : null}
    </Dialog>
  );
}

function DeleteDialog({ meeting: m, onClose }: { meeting: MeetingDetail; onClose: () => void }): ReactElement {
  const t = useT();
  const { show } = useToast();
  const [busy, setBusy] = useState(false);
  const confirm = async () => {
    setBusy(true);
    try {
      await deleteMeeting(m.id);
      onClose();
      navigate('meetings');
    } catch (e) {
      show({ message: errorMessage(t, e) });
      setBusy(false);
      onClose();
    }
  };
  return (
    <Dialog
      title={t('meetings.deleteTitle', { title: m.title })}
      role="alertdialog"
      onClose={onClose}
      actions={
        <>
          <Button variant="quiet" autoFocus onClick={onClose}>{t('common.cancel')}</Button>
          <Button variant="secondary" disabled={busy} onClick={() => void confirm()}>{t('meetings.delete')}</Button>
        </>
      }
    >
      <p className="body mt-m0">{t('meetings.deleteBody')}</p>
    </Dialog>
  );
}
