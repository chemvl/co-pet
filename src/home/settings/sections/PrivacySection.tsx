import { Switch } from '../../../ds';
import { useT } from '../../../shared/i18n';
import { useAiStatus, useSetSetting, useSetting } from '../../../shared/queries';
import { activeCloud, providerName } from '../../integrations/cloudModel';
import { Group, SettingRow } from '../parts';

const RETENTION_DAYS = [7, 30, 90];

export function PrivacySection({ onNavigate }: { onNavigate: (route: string) => void }) {
  const t = useT();
  const set = useSetSetting();
  const secrets = useSetting<boolean>('privacy.secretDetection', true);
  const localOnly = useSetting<boolean>('privacy.localOnly', false);
  const cloudMeetings = useSetting<boolean>('ai.cloudMeetings', false);
  const { data: ai } = useAiStatus();
  const cloud = activeCloud(ai);
  const retention = useSetting<number | null>('transcribe.audioRetentionDays', 30);
  return (
    <>
      <Group title={t('settings.privacy')}>
        <Switch
          id="st-secrets"
          label={t('settings.privacy.secrets')}
          description={t('settings.privacy.secretsDesc')}
          checked={secrets}
          onChange={(v) => void set('privacy.secretDetection', v)}
        />
        <Switch
          id="st-local-only"
          label={t('settings.privacy.localOnly')}
          description={t('settings.privacy.localOnlyDesc')}
          checked={localOnly}
          onChange={(v) => void set('privacy.localOnly', v)}
        />
        {cloud ? (
          <p className="st-body">
            {t('settings.privacy.cloudNote', { provider: providerName(cloud.id) })}{' '}
            {t(cloudMeetings ? 'settings.privacy.cloudMeetingsOn' : 'settings.privacy.cloudMeetingsOff')}
          </p>
        ) : null}
        <SettingRow label={t('settings.privacy.keepAudio')} desc={t('settings.privacy.keepAudioDesc')} htmlFor="st-keep-audio">
          <select
            id="st-keep-audio"
            className="st-input"
            value={retention ?? ''}
            onChange={(e) => void set('transcribe.audioRetentionDays', e.currentTarget.value ? Number(e.currentTarget.value) : null)}
          >
            {RETENTION_DAYS.map((n) => (
              <option key={n} value={n}>
                {t('settings.privacy.days', { n })}
              </option>
            ))}
            <option value="">{t('settings.privacy.keepForever')}</option>
          </select>
        </SettingRow>
      </Group>
      <Group title={t('settings.capture.excluded')} desc={t('settings.privacy.excludedHint')}>
        <button type="button" className="st-link" onClick={() => onNavigate('settings/capture')}>
          {t('settings.privacy.excludedLink')}
        </button>
      </Group>
    </>
  );
}
