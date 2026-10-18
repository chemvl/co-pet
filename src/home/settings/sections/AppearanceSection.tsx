import { Button, SegmentedControl } from '../../../ds';
import { useT } from '../../../shared/i18n';
import { useSetSetting, useSetting } from '../../../shared/queries';
import { Group, SettingRow } from '../parts';

export function AppearanceSection({ onNavigate }: { onNavigate: (route: string) => void }) {
  const t = useT();
  const set = useSetSetting();
  const theme = useSetting<string>('ui.theme', 'system');

  return (
    <Group title={t('settings.appearance')}>
      <SettingRow label={t('settings.appearance.pet')} desc={t('settings.appearance.petDesc')}>
        <Button size="sm" onClick={() => onNavigate('pet')}>
          {t('settings.appearance.openPetRoom')}
        </Button>
      </SettingRow>
      <SettingRow
        label={t('settings.appearance.theme')}
        desc={t('settings.appearance.themeDesc')}
      >
        <SegmentedControl
          label={t('settings.appearance.theme')}
          value={theme === 'light' || theme === 'dark' ? theme : 'system'}
          onChange={(v) => void set('ui.theme', v)}
          options={[
            { value: 'system', label: t('common.system'), icon: 'monitor' },
            { value: 'light', label: t('settings.appearance.themeLight'), icon: 'sun' },
            { value: 'dark', label: t('settings.appearance.themeDark'), icon: 'moon' },
          ]}
        />
      </SettingRow>
    </Group>
  );
}
