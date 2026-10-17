import { getVersion } from '@tauri-apps/api/app';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useQuery } from '@tanstack/react-query';
import { Button, Switch } from '../../../ds';
import { petLookOf } from '../../../pet/catalog';
import { errorMessage } from '../../../shared/errors';
import { useT } from '../../../shared/i18n';
import { checkForUpdates, installUpdate } from '../../../shared/ipc';
import { useSetSetting, useSetting, useSettings, useUpdateStatus } from '../../../shared/queries';
import { useToast } from '../../../shared/Toast';
import { Group, StillCat } from '../parts';
import { updateLine } from '../updateText';

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
  const set = useSetSetting();
  const autoCheck = useSetting('updates.autoCheck', true);
  const update = useUpdateStatus().data;
  const state = update?.state ?? 'idle';
  const line = update ? updateLine(update, t) : null;
  const fail = (e: unknown) => toast.show({ message: errorMessage(t, e) });
  const check = () => void checkForUpdates().catch(fail);
  const install = () => void installUpdate().catch(fail);
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
            <p className="st-desc">{version.data ? t('settings.about.version', { version: version.data }) : '\u00a0'}</p>
          </div>
        </div>
        <p className="st-body">{t('settings.about.tagline')}</p>
      </Group>
      <Group title={t('settings.about.updates')}>
        {line && <p className="st-desc">{line}</p>}
        {state === 'ready' ? (
          <Button variant="primary" onClick={install}>{t('settings.about.restart')}</Button>
        ) : (
          <Button variant="quiet" disabled={state === 'checking' || state === 'downloading'} onClick={check}>
            {t('settings.about.check')}
          </Button>
        )}
        {update?.notes && (
          <>
            <h3 className="st-label">{t('settings.about.notes')}</h3>
            <p className="st-body">{update.notes}</p>
          </>
        )}
        <Switch
          id="st-auto-update"
          label={t('settings.about.autoCheck')}
          description={t('settings.about.autoCheckDesc')}
          checked={autoCheck}
          onChange={(v) => void set('updates.autoCheck', v)}
        />
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
