import { Switch } from '../../../ds';
import { LANG_LABEL, LANGS, useT } from '../../../shared/i18n';
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
        <SettingRow label={t('settings.language')} desc={t('settings.general.languageDesc')} htmlFor="st-language">
          <select
            id="st-language"
            className="st-input"
            value={language}
            onChange={(e) => void set('ui.language', e.currentTarget.value)}
          >
            <option value="system">{t('common.system')}</option>
            {LANGS.map((l) => (
              <option key={l} value={l}>
                {t(LANG_LABEL[l])}
              </option>
            ))}
          </select>
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
