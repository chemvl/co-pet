import { getVersion } from '@tauri-apps/api/app';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useQuery } from '@tanstack/react-query';
import { petLookOf } from '../../../pet/catalog';
import { errorMessage } from '../../../shared/errors';
import { useT } from '../../../shared/i18n';
import { isWindows } from '../../../shared/platform';
import { useSettings } from '../../../shared/queries';
import { useToast } from '../../../shared/Toast';
import { Group, StillCat } from '../parts';

const LINKS = [
  ['settings.about.privacy', 'https://mrdeeds88.github.io/copet/privacy/'],
  ['settings.about.terms', 'https://mrdeeds88.github.io/copet/terms/'],
  ['settings.about.support', 'https://mrdeeds88.github.io/copet/support/'],
] as const;

export function AboutSection() {
  const t = useT();
  const version = useQuery({ queryKey: ['appVersion'], queryFn: getVersion, staleTime: Infinity });
  const { species, skin } = petLookOf(useSettings().data);
  const toast = useToast();
  const fail = (e: unknown) => toast.show({ message: errorMessage(t, e) });
  const versionLine = version.data
    ? [t('settings.about.version', { version: version.data }), ...(isWindows ? [t('settings.about.storeUpdates')] : [])].join(' · ')
    : '\u00a0';
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
          <span className="st-about__art"><StillCat look={species} skin={skin} /></span>
          <div>
            <div className="pet-name">{t('app.name')}</div>
            <p className="st-desc">{versionLine}</p>
          </div>
        </div>
        <p className="st-body">{t('settings.about.tagline')}</p>
      </Group>
      <Group title={t('settings.about.links')}>
        {LINKS.map(([key, url]) => (
          <button key={key} type="button" className="hm-link" onClick={() => void openUrl(url).catch(fail)}>
            {t(key)}
          </button>
        ))}
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
