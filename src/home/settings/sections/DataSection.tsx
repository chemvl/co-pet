import { open } from '@tauri-apps/plugin-dialog';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { useState } from 'react';
import { Button } from '../../../ds';
import { errorText } from '../../../shared/errors';
import { useT } from '../../../shared/i18n';
import { deleteEverything, exportAll } from '../../../shared/ipc';
import { useToast } from '../../../shared/Toast';
import { Group, SettingRow, TypedConfirmDialog } from '../parts';

export function DataSection() {
  const t = useT();
  const toast = useToast();
  const [exporting, setExporting] = useState(false);
  const [confirming, setConfirming] = useState(false);

  const runExport = async () => {
    const dir = await open({ directory: true, multiple: false, title: t('settings.data.exportPick') });
    if (typeof dir !== 'string') return;
    setExporting(true);
    try {
      const path = await exportAll(dir);
      const folder = path.split('/').filter(Boolean).pop() ?? path;
      toast.show({
        message: t('settings.data.exported', { folder }),
        action: { label: t('common.showInFinder'), run: () => void revealItemInDir(path) },
      });
    } catch (e) {
      toast.show({ message: t('settings.data.exportFailed', { reason: errorText(e) }) });
    } finally {
      setExporting(false);
    }
  };

  return (
    <>
      <Group title={t('settings.data')}>
        <SettingRow label={t('settings.data.export')} desc={t('settings.data.exportDesc')}>
          <Button size="sm" icon="export" disabled={exporting} onClick={() => void runExport()}>
            {t('settings.data.export')}
          </Button>
        </SettingRow>
      </Group>
      <div className="st-danger">
        <div className="st-danger__text">
          <b>{t('settings.data.delete')}</b>
          {t('settings.data.deleteDesc')}
        </div>
        <Button variant="danger" size="sm" icon="trash" onClick={() => setConfirming(true)}>
          {t('settings.data.delete')}
        </Button>
      </div>
      {confirming ? (
        <TypedConfirmDialog
          title={t('settings.data.deleteTitle')}
          body={t('settings.data.deleteBody')}
          phrase={t('settings.data.deletePhrase')}
          action={t('settings.data.delete')}
          variant="danger"
          // Rust stops the worker and the model, deletes the library and restarts the app.
          onConfirm={deleteEverything}
          failed={(e) => t('settings.data.deleteFailed', { reason: errorText(e) })}
          onClose={() => setConfirming(false)}
        />
      ) : null}
    </>
  );
}
