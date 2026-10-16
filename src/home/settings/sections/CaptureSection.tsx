import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Button, Icon, Switch } from '../../../ds';
import { useT } from '../../../shared/i18n';
import { listExcludedApps, pickExcludedApp } from '../../../shared/ipc';
import { SETTINGS_KEY, useSetSetting, useSetting } from '../../../shared/queries';
import { useToast } from '../../../shared/Toast';
import { errorText, Group } from '../parts';

export function CaptureSection() {
  const t = useT();
  const set = useSetSetting();
  const qc = useQueryClient();
  const toast = useToast();
  const removeOriginal = useSetting<boolean>('capture.removeOriginal', false);
  const excluded = useSetting<string[]>('capture.excludedApps', []);
  // Keyed on the setting so adds/removes (here or from Rust) refetch the names.
  const apps = useQuery({
    queryKey: ['excludedApps', excluded],
    queryFn: listExcludedApps,
    placeholderData: (prev) => prev,
  });

  const remove = (bundleId: string) =>
    void set('capture.excludedApps', excluded.filter((id) => id !== bundleId));

  const add = async () => {
    try {
      await pickExcludedApp();
    } catch (e) {
      toast.show({ message: t('settings.capture.addFailed', { reason: errorText(e) }) });
    } finally {
      void qc.invalidateQueries({ queryKey: SETTINGS_KEY });
    }
  };

  return (
    <>
      <Group title={t('settings.capture')}>
        <Switch
          id="st-remove-original"
          label={t('settings.capture.removeOriginal')}
          description={t('settings.capture.removeOriginalDesc')}
          checked={removeOriginal}
          onChange={(v) => void set('capture.removeOriginal', v)}
        />
      </Group>
      <Group title={t('settings.capture.excluded')} desc={t('settings.capture.excludedDesc')}>
        {apps.data && apps.data.length > 0 ? (
          <ul className="st-apps">
            {apps.data.map((app) => {
              const name = app.name ?? app.bundleId;
              return (
                <li key={app.bundleId} className="st-app">
                  <Icon name="lock" size={16} />
                  <span className="st-app__text">
                    <span className="st-app__name">{name}</span>
                    {app.name ? (
                      <span className="st-app__id">{app.bundleId}</span>
                    ) : (
                      <span className="st-app__note">{t('settings.capture.notInstalled')}</span>
                    )}
                  </span>
                  <Button
                    variant="quiet"
                    size="sm"
                    icon="close"
                    aria-label={t('settings.capture.removeApp', { app: name })}
                    onClick={() => remove(app.bundleId)}
                  />
                </li>
              );
            })}
          </ul>
        ) : apps.data ? (
          <p className="st-body">{t('settings.capture.noExcluded')}</p>
        ) : null}
        <Button size="sm" icon="plus" style={{ alignSelf: 'flex-start' }} onClick={() => void add()}>
          {t('settings.capture.addApp')}
        </Button>
      </Group>
    </>
  );
}
