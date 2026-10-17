import { useT, type I18nKey } from '../../../shared/i18n';
import { useAiStatus, useSetSetting, useSetting } from '../../../shared/queries';
import type { MeetingLanguage } from '../../../shared/types';
import { activeCloud, providerName } from '../../integrations/cloudModel';
import { Group, SettingRow } from '../parts';
import { SpeechModelCards } from '../SpeechModelCards';

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
      <p className="st-body">
        {cloud && cloudMeetings
          ? t('settings.transcribe.captionCloud', { provider: providerName(cloud.id) })
          : t('settings.transcribe.caption')}
      </p>
    </>
  );
}
