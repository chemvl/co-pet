import { describe, expect, it } from 'vitest';
import { translate } from '../shared/i18n';
import type { PetInput } from '../shared/types';
import { canSpeak, createSpeechLimiter, SPEECH_LINES, speechFor, type SpeechContext, type SpeechLine } from './speech';

describe('speech', () => {
  it('maps reactions to lines', () => {
    const direct: PetInput[] = ['catch', 'camera', 'idea', 'tilt', 'celebrate', 'cheer', 'confused'];
    for (const input of direct) expect(speechFor({ input })).toBe(input);
    for (const input of ['glance', 'wake', 'gesture', 'handOver', 'poof'] as PetInput[]) expect(speechFor({ input })).toBeNull();
    expect(speechFor({ input: 'fidget' })).toBeNull();
    for (const variant of [0, 1, 2]) expect(speechFor({ input: 'fidget', variant })).toBeNull();
    expect(speechFor({ input: 'fidget', variant: 3 })).toBe('duplicate');
  });

  it('every line fits 40 characters with no exclamation mark', () => {
    const cases = [
      ['en', { category: 'Reference' }],
      ['vi', { category: 'Tham khảo' }],
      ['zh', { category: '参考资料' }],
      ['es', { category: 'Referencia' }],
    ] as const;
    for (const [lang, params] of cases) {
      for (const line of Object.keys(SPEECH_LINES) as SpeechLine[]) {
        const text = translate(lang, SPEECH_LINES[line].key, params);
        expect(text.length, `${lang} ${line}: ${text}`).toBeLessThanOrEqual(40);
        expect(text).not.toContain('!');
        expect(text).not.toContain('{');
      }
    }
  });

  it('canSpeak needs the switch and nothing in the way', () => {
    const base: SpeechContext = {
      enabled: true, paused: false, hidden: false, asleep: false,
      surfaceOpen: false, recording: false, moving: false, dragOver: false,
    };
    expect(canSpeak(base)).toBe(true);
    for (const k of Object.keys(base) as (keyof SpeechContext)[]) {
      if (k === 'enabled') continue;
      expect(canSpeak({ ...base, [k]: true }), k).toBe(false);
    }
    expect(canSpeak({ ...base, enabled: false })).toBe(false);
  });

  it('limiter keeps 20 s between lines', () => {
    const l = createSpeechLimiter();
    expect(l.take('catch', 0)).toBe(true);
    expect(l.take('cheer', 19_999)).toBe(false);
    expect(l.take('cheer', 20_000)).toBe(true);
  });

  it('limiter caps 12 an hour', () => {
    const l = createSpeechLimiter();
    for (let i = 0; i < 12; i++) expect(l.take('catch', i * 20_000)).toBe(true);
    expect(l.take('catch', 240_000)).toBe(false);
    expect(l.take('catch', 3_600_000 + 1)).toBe(true);
  });

  it('newLook is never limited and does not count', () => {
    const l = createSpeechLimiter();
    expect(l.take('newLook', 0)).toBe(true);
    expect(l.take('newLook', 1)).toBe(true);
    expect(l.take('newLook', 2)).toBe(true);
    expect(l.take('catch', 3)).toBe(true);
  });
});
