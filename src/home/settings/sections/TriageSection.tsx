import { Icon, SegmentedControl, Switch } from '../../../ds';
import { formatSize } from '../../../shared/format';
import { useLang, useT } from '../../../shared/i18n';
import { useAiStatus, useSetSetting, useSetting } from '../../../shared/queries';
import { activeCloud, providerName } from '../../integrations/cloudModel';
import { ModelCards } from '../ModelCards';
import { Group, SettingRow } from '../parts';

const THRESHOLDS = [0.4, 0.6, 0.8] as const;

export function TriageSection() {
  const t = useT();
  const lang = useLang();
  const set = useSetSetting();
  const { data: status } = useAiStatus();
  const aiEnabled = useSetting<boolean>('ai.enabled', true);
  const threshold = useSetting<number>('triage.confidenceThreshold', 0.6);
  const activeId = useSetting<string | null>('ai.modelId', null);
  const localOnly = useSetting<boolean>('privacy.localOnly', false);

  const nearest = THRESHOLDS.reduce((a, b) => (Math.abs(b - threshold) < Math.abs(a - threshold) ? b : a));
  const ready = !!status?.models.some((m) => m.id === activeId && m.downloaded);
  // Size of the model being fetched, else the recommended one.
  const pending = status?.models.find((m) => m.id === (status.modelId ?? status.recommendedId));
  const cloud = activeCloud(status);
  // Local-only mode turns cloud providers off, so the provider is not presented as in use.
  const providerLabel = cloud && localOnly
    ? t('integrations.ai.localOnly')
    : cloud
    ? `${providerName(cloud.id)}${cloud.ready ? '' : ` · ${t('integrations.ai.notSetUp')}`}`
    : t('integrations.ai.local');

  return (
    <>
      <Group title={t('settings.triage.model')} desc={t('settings.triage.modelDesc')}>
        {(!ready && status?.cloud.provider === 'local') || (cloud && localOnly) ? (
          <p className="st-info">
            <Icon name="idea" size={20} />
            <span>
              {localOnly && (cloud || !ready)
                ? t('settings.triage.localOnlyBlocked')
                : t('ai.rulesUntilDownload', { size: pending ? formatSize(pending.bytes, lang) : '' })}
            </span>
          </p>
        ) : null}
        <ModelCards />
      </Group>
      <Group title={t('settings.triage')}>
        <SettingRow label={t('settings.triage.provider')} desc={providerLabel}>
          <a className="hm-link" href="#/integrations">{t('settings.triage.changeProvider')}</a>
        </SettingRow>
        <Switch
          id="st-ai-enabled"
          label={t('settings.triage.aiEnabled')}
          description={t('settings.triage.aiEnabledDesc')}
          checked={aiEnabled}
          onChange={(v) => void set('ai.enabled', v)}
        />
        <SettingRow label={t('settings.triage.threshold')} desc={t('settings.triage.thresholdDesc')}>
          <SegmentedControl
            label={t('settings.triage.threshold')}
            value={String(nearest)}
            onChange={(v) => void set('triage.confidenceThreshold', Number(v))}
            options={[
              { value: '0.4', label: t('settings.triage.low') },
              { value: '0.6', label: t('settings.triage.medium') },
              { value: '0.8', label: t('settings.triage.high') },
            ]}
          />
        </SettingRow>
      </Group>
    </>
  );
}
