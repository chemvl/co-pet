import { getVersion } from '@tauri-apps/api/app';
import { useQuery } from '@tanstack/react-query';
import { useT } from '../../../shared/i18n';
import { useSetting } from '../../../shared/queries';
import { asLook, asSkin, Group, StillCat } from '../parts';

export function AboutSection() {
  const t = useT();
  const version = useQuery({ queryKey: ['appVersion'], queryFn: getVersion, staleTime: Infinity });
  const look = asLook(useSetting<string>('pet.style', 'pixel'));
  const skin = asSkin(useSetting<string>('pet.skin', 'peach'));
  const licenses: [string, string][] = [
    ['Co-pet', t('settings.about.copet')],
    ['llama.cpp', t('settings.about.mit')],
    ['Qwen3', t('settings.about.apache')],
    [t('settings.about.fonts'), t('settings.about.ofl')],
  ];
  return (
    <>
      <Group title={t('settings.about')}>
        <div className="st-about">
          <span className="st-about__art"><StillCat look={look} skin={skin} /></span>
          <div>
            <div className="pet-name">{t('app.name')}</div>
            <p className="st-desc">{version.data ? t('settings.about.version', { version: version.data }) : '\u00a0'}</p>
          </div>
        </div>
        <p className="st-body">{t('settings.about.tagline')}</p>
      </Group>
      <Group title={t('settings.about.licenses')}>
        <ul className="st-licenses">
          {licenses.map(([name, license]) => (
            <li key={name}>
              <b>{name}</b>
              <span>{license}</span>
            </li>
          ))}
        </ul>
      </Group>
    </>
  );
}
