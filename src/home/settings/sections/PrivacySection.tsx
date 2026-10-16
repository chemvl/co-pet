import { Switch } from '../../../ds';
import { useT } from '../../../shared/i18n';
import { useSetSetting, useSetting } from '../../../shared/queries';
import { Group } from '../parts';

export function PrivacySection({ onNavigate }: { onNavigate: (route: string) => void }) {
  const t = useT();
  const set = useSetSetting();
  const secrets = useSetting<boolean>('privacy.secretDetection', true);
  const localOnly = useSetting<boolean>('privacy.localOnly', false);
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
      </Group>
      <Group title={t('settings.capture.excluded')} desc={t('settings.privacy.excludedHint')}>
        <button type="button" className="st-link" onClick={() => onNavigate('settings/capture')}>
          {t('settings.privacy.excludedLink')}
        </button>
      </Group>
    </>
  );
}
