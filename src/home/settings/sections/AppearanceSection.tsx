import { useEffect, useState } from 'react';
import { SegmentedControl } from '../../../ds';
import { useT } from '../../../shared/i18n';
import { useSetSetting, useSetting } from '../../../shared/queries';
import { asLook, asSkin, Group, SettingRow, SkinSwatches, StyleTiles } from '../parts';

const NAME_MAX = 24;

export function AppearanceSection() {
  const t = useT();
  const set = useSetSetting();
  const savedName = useSetting<string>('pet.name', 'Miso');
  const look = asLook(useSetting<string>('pet.style', 'soft'));
  const skin = asSkin(useSetting<string>('pet.skin', 'peach'));
  const size = useSetting<number>('pet.size', 80);
  const [name, setName] = useState(savedName);
  useEffect(() => {
    setName(savedName);
  }, [savedName]);

  const commitName = () => {
    const next = name.trim();
    if (!next) setName(savedName);
    else if (next !== savedName) void set('pet.name', next);
  };

  return (
    <>
      <Group title={t('settings.appearance')}>
        <SettingRow label={t('settings.appearance.name')} desc={t('settings.appearance.nameDesc')} htmlFor="st-pet-name">
          <input
            id="st-pet-name"
            className="st-input st-input--wide"
            value={name}
            maxLength={NAME_MAX}
            onChange={(e) => setName(e.target.value)}
            onBlur={commitName}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commitName();
              if (e.key === 'Escape') setName(savedName);
            }}
          />
        </SettingRow>
      </Group>
      <Group title={t('settings.appearance.style')} desc={t('settings.appearance.styleDesc')}>
        <StyleTiles value={look} skin={skin} onChange={(v) => void set('pet.style', v)} />
      </Group>
      <Group title={t('settings.appearance.skin')} desc={t('settings.appearance.skinDesc')}>
        <SkinSwatches value={skin} onChange={(v) => void set('pet.skin', v)} />
      </Group>
      <Group>
        <SettingRow label={t('settings.appearance.size')} desc={t('settings.appearance.sizeDesc')}>
          <SegmentedControl
            label={t('settings.appearance.size')}
            value={String(size)}
            onChange={(v) => void set('pet.size', Number(v))}
            options={[
              { value: '64', label: t('settings.appearance.sizeSmall') },
              { value: '80', label: t('settings.appearance.sizeMedium') },
              { value: '96', label: t('settings.appearance.sizeLarge') },
            ]}
          />
        </SettingRow>
      </Group>
      <Group title={t('settings.appearance.theme')}>
        <p className="st-body">{t('settings.appearance.themeDesc')}</p>
      </Group>
    </>
  );
}
