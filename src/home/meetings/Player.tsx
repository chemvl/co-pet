// The meeting's playback bar (Meetings.dc.html `.player`): play/pause, current time, a seek track
// and the duration. `usePlayer` owns the <audio> state so transcript lines can seek and play.
import { convertFileSrc } from '@tauri-apps/api/core';
import { useRef, useState, type CSSProperties, type ReactElement, type RefObject, type SyntheticEvent } from 'react';
import { Button } from '../../ds';
import { formatClock } from '../../shared/format';
import { useT } from '../../shared/i18n';

export interface PlayerState {
  playing: boolean;
  atMs: number;
  durationMs: number;
  toggle(): void;
  /** Moves to `ms`; `play` also starts playback. */
  seek(ms: number, play?: boolean): void;
  audioProps: {
    ref: RefObject<HTMLAudioElement>;
    onPlay(): void;
    onPause(): void;
    onTimeUpdate(e: SyntheticEvent<HTMLAudioElement>): void;
    onLoadedMetadata(e: SyntheticEvent<HTMLAudioElement>): void;
  };
}

/** `totalMs` stands in for the duration until the audio's metadata loads; `onError` gets
 *  playback failures. */
export function usePlayer(totalMs: number, onError: (e: unknown) => void): PlayerState {
  const ref = useRef<HTMLAudioElement>(null);
  const [playing, setPlaying] = useState(false);
  const [atMs, setAtMs] = useState(0);
  const [mediaMs, setMediaMs] = useState<number | null>(null);
  const play = (a: HTMLAudioElement) => void a.play().catch(onError);
  return {
    playing,
    atMs,
    durationMs: mediaMs ?? totalMs,
    toggle() {
      const a = ref.current;
      if (!a) return;
      if (a.paused) play(a);
      else a.pause();
    },
    seek(ms, andPlay) {
      const a = ref.current;
      if (!a) return;
      a.currentTime = ms / 1000;
      setAtMs(ms);
      if (andPlay && a.paused) play(a);
    },
    audioProps: {
      ref,
      onPlay: () => setPlaying(true),
      onPause: () => setPlaying(false),
      onTimeUpdate: (e) => setAtMs(e.currentTarget.currentTime * 1000),
      onLoadedMetadata: (e) => {
        const d = e.currentTarget.duration;
        if (Number.isFinite(d)) setMediaMs(d * 1000);
      },
    },
  };
}

export function Player({ player, audioPath }: { player: PlayerState; audioPath: string }): ReactElement {
  const t = useT();
  const { playing, atMs, durationMs } = player;
  const at = Math.min(atMs, durationMs);
  const pos = durationMs > 0 ? (at / durationMs) * 100 : 0;
  return (
    <div className="mt-player">
      <audio {...player.audioProps} src={convertFileSrc(audioPath)} preload="metadata" />
      <Button
        variant="secondary"
        size="sm"
        icon={playing ? 'pause' : 'play'}
        aria-label={t(playing ? 'meetings.pause' : 'meetings.play')}
        onClick={player.toggle}
      />
      <span className="mt-time">{formatClock(at)}</span>
      <input
        type="range"
        className="mt-track"
        min={0}
        max={Math.max(durationMs, 1)}
        step={100}
        value={at}
        style={{ '--mt-pos': `${pos}%` } as CSSProperties}
        aria-label={t('meetings.position', { at: formatClock(at), total: formatClock(durationMs) })}
        onChange={(e) => player.seek(Number(e.currentTarget.value))}
      />
      <span className="mt-time">{formatClock(durationMs)}</span>
    </div>
  );
}
