import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { Button, SegmentedControl, Switch } from '../../../ds';
import { useT, type I18nKey } from '../../../shared/i18n';
import { callDetectionStatus, requestPermission } from '../../../shared/ipc';
import { isWindows } from '../../../shared/platform';
import { useAiStatus, useSetSetting, useSetting } from '../../../shared/queries';
import type { MeetingLanguage } from '../../../shared/types';
import { activeCloud, providerName } from '../../integrations/cloudModel';
import { Dialog, Group, SettingRow } from '../parts';
import { SpeechModelCards } from '../SpeechModelCards';
import { callsStatusLine } from './calls';

type CallMode = 'off' | 'ask' | 'auto';

const CALL_MODES: readonly { value: CallMode; label: I18nKey }[] = [
  { value: 'off', label: 'settings.calls.mode.off' },
  { value: 'ask', label: 'settings.calls.mode.ask' },
  { value: 'auto', label: 'settings.calls.mode.auto' },
];

const MEETING_LANGUAGES: readonly { value: MeetingLanguage; label: I18nKey }[] = [
  { value: 'auto', label: 'lang.auto' },
  { value: 'en', label: 'lang.en' },
  { value: 'vi', label: 'lang.vi' },
  { value: 'zh', label: 'lang.zh' },
  { value: 'vi+en', label: 'lang.viEn' },
];

/** `meeting` follows the meeting's language. */
const SUMMARY_LANGUAGES: readonly { value: 'meeting' | 'en' | 'vi' | 'zh'; label: I18nKey }[] = [
  { value: 'meeting', label: 'settings.transcribe.sameAsMeeting' },
  { value: 'en', label: 'lang.en' },
  { value: 'vi', label: 'lang.vi' },
  { value: 'zh', label: 'lang.zh' },
];

export function TranscribeSection() {
  const t = useT();
  const { data: ai } = useAiStatus();
  const cloud = activeCloud(ai);
  const cloudMeetings = useSetting<boolean>('ai.cloudMeetings', false);
  const set = useSetSetting();
  const language = useSetting<MeetingLanguage>('transcribe.language', 'auto');
  const summaryLanguage = useSetting<string>('transcribe.summaryLanguage', 'meeting');

  return (
    <>
      <Group title={t('settings.transcribe')}>
        <SpeechModelCards />
        <SettingRow label={t('settings.transcribe.language')} desc={t('settings.transcribe.languageDesc')} htmlFor="st-tr-lang">
          <select
            id="st-tr-lang"
            className="st-input"
            value={language}
            onChange={(e) => void set('transcribe.language', e.currentTarget.value)}
          >
            {MEETING_LANGUAGES.map((o) => (
              <option key={o.value} value={o.value}>
                {t(o.label)}
              </option>
            ))}
          </select>
        </SettingRow>
        <SettingRow label={t('settings.transcribe.summaryLanguage')} htmlFor="st-tr-summary">
          <select
            id="st-tr-summary"
            className="st-input"
            value={summaryLanguage}
            onChange={(e) => void set('transcribe.summaryLanguage', e.currentTarget.value)}
          >
            {SUMMARY_LANGUAGES.map((o) => (
              <option key={o.value} value={o.value}>
                {t(o.label)}
              </option>
            ))}
          </select>
        </SettingRow>
      </Group>
      <CallsGroup />
      <p className="st-body">
        {cloud && cloudMeetings
          ? t('settings.transcribe.captionCloud', { provider: providerName(cloud.id) })
          : t('settings.transcribe.caption')}
      </p>
    </>
  );
}

function CallsGroup() {
  const t = useT();
  const set = useSetSetting();
  const mode = useSetting<CallMode>('transcribe.autoRecord', 'ask');
  const autoStop = useSetting<boolean>('transcribe.autoStop', true);
  const [confirmAuto, setConfirmAuto] = useState(false);
  const status = useQuery({ queryKey: ['calls', 'status'], queryFn: callDetectionStatus, refetchOnWindowFocus: true });
  const line = status.data ? callsStatusLine(status.data, t) : null;

  const choose = (value: string) => {
    if (value === mode) return;
    if (value === 'auto') setConfirmAuto(true);
    else void set('transcribe.autoRecord', value);
  };
  const turnOnAuto = async () => {
    setConfirmAuto(false);
    await set('transcribe.consentShown', true);
    await set('transcribe.autoRecord', 'auto');
  };

  return (
    <Group title={t('settings.calls')} desc={t('settings.calls.desc')}>
      <SettingRow label={t('settings.calls.mode')}>
        <SegmentedControl
          label={t('settings.calls.mode')}
          value={mode}
          options={CALL_MODES.map((o) => ({ value: o.value, label: t(o.label) }))}
          onChange={choose}
        />
      </SettingRow>
      {/* Off: the switch sits at 45% with the reason beside it. */}
      <fieldset className="st-fieldset" disabled={mode === 'off'}>
        <Switch
          id="st-calls-autostop"
          label={t('settings.calls.autoStop')}
          description={t('settings.calls.autoStop.desc')}
          checked={autoStop}
          onChange={(v) => void set('transcribe.autoStop', v)}
        />
      </fieldset>
      {mode === 'off' && <p className="st-desc">{t('settings.calls.autoStop.off')}</p>}
      {line && (
        <>
          <p className="st-body">{line.text}</p>
          {line.needsPermission && !isWindows && (
            <Button size="sm" variant="secondary" onClick={() => void requestPermission('screenRecording')}>
              {t('common.openSettings')}
            </Button>
          )}
        </>
      )}
      {confirmAuto && (
        <Dialog
          title={t('settings.calls.autoConsent.title')}
          onClose={() => setConfirmAuto(false)}
          actions={
            <>
              <Button variant="quiet" onClick={() => setConfirmAuto(false)}>{t('common.cancel')}</Button>
              <Button variant="primary" autoFocus onClick={() => void turnOnAuto()}>
                {t('settings.calls.autoConsent.turnOn')}
              </Button>
            </>
          }
        >
          <p className="st-body">{t('settings.calls.autoConsent.body')}</p>
        </Dialog>
      )}
    </Group>
  );
}
