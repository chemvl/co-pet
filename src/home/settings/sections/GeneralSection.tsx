import { SegmentedControl, Switch } from '../../../ds';
import { useT } from '../../../shared/i18n';
import { useSetSetting, useSetting } from '../../../shared/queries';
import { HotkeyRows } from '../HotkeyRows';
import { Group, SettingRow } from '../parts';

export function GeneralSection() {
  const t = useT();
  const set = useSetSetting();
  const language = useSetting<string>('ui.language', 'system');
  const openAtLogin = useSetting<boolean>('general.openAtLogin', true);
  return (
    <>
      <Group title={t('settings.general')}>
        <SettingRow label={t('settings.language')} desc={t('settings.general.languageDesc')}>
          <SegmentedControl
            label={t('settings.language')}
            value={language}
            onChange={(v) => void set('ui.language', v)}
            options={[
              { value: 'system', label: t('common.system') },
              { value: 'en', label: t('common.english') },
              { value: 'vi', label: t('common.vietnamese') },
            ]}
          />
        </SettingRow>
        <Switch
          id="st-open-at-login"
          label={t('settings.general.openAtLogin')}
          description={t('settings.general.openAtLoginDesc')}
          checked={openAtLogin}
          onChange={(v) => void set('general.openAtLogin', v)}
        />
      </Group>
      <Group title={t('settings.hotkeys.title')} desc={t('settings.hotkeys.desc')}>
        <HotkeyRows />
      </Group>
    </>
  );
}
