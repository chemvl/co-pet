// Windows: where the local models and the data (database, captured files, recordings) live.
// Models move live; a data move waits for the next start (Settings restarts, onboarding's Finish does).
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { open } from '@tauri-apps/plugin-dialog';
import { useState } from 'react';
import { Button } from '../../ds';
import { errorMessage } from '../../shared/errors';
import { useT } from '../../shared/i18n';
import { cancelDataMove, restartApp, setDataDir, setModelsDir, storageLocations } from '../../shared/ipc';
import { AI_STATUS_KEY, SPEECH_STATUS_KEY, STORAGE_KEY } from '../../shared/queries';
import { useToast } from '../../shared/Toast';
import type { StorageLocations } from '../../shared/types';
import { Dialog, ProgressBar, SettingRow } from './parts';

export function StorageRows({ mode }: { mode: 'settings' | 'onboarding' }) {
  const t = useT();
  const qc = useQueryClient();
  const toast = useToast();
  const { data } = useQuery({ queryKey: STORAGE_KEY, queryFn: storageLocations });
  const [moving, setMoving] = useState<'models' | 'data' | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);

  if (!data) return null;
  const saved = (next: StorageLocations) => qc.setQueryData(STORAGE_KEY, next);

  /** Runs `move` with the picked folder; cancelling the picker does nothing. */
  const pickThen = async (title: string, defaultPath: string, move: (dir: string) => Promise<void>) => {
    const dir = await open({ directory: true, multiple: false, title, defaultPath });
    if (typeof dir === 'string') await move(dir);
  };

  const moveModels = async (dir: string | null) => {
    setMoving('models');
    try {
      const next = await setModelsDir(dir);
      saved(next);
      toast.show({ message: t('storage.modelsMoved', { path: next.modelsDir }) });
    } catch (e) {
      toast.show({ message: t('storage.moveFailed', { reason: errorMessage(t, e) }) });
    } finally {
      setMoving(null);
      void qc.invalidateQueries({ queryKey: AI_STATUS_KEY });
      void qc.invalidateQueries({ queryKey: SPEECH_STATUS_KEY });
    }
  };

  const moveData = async (dir: string | null) => {
    setMoving('data');
    try {
      const next = await setDataDir(dir);
      saved(next);
      if (mode === 'settings' && next.pendingDataDir) setConfirming(next.pendingDataDir);
    } catch (e) {
      toast.show({ message: errorMessage(t, e) });
    } finally {
      setMoving(null);
    }
  };

  const cancelMove = async () => {
    setConfirming(null);
    try {
      saved(await cancelDataMove());
    } catch (e) {
      toast.show({ message: errorMessage(t, e) });
    }
  };

  // A refusal (`recording`) keeps the move pending; the row offers Restart now.
  const restart = async () => {
    setConfirming(null);
    try {
      await restartApp();
    } catch (e) {
      toast.show({ message: errorMessage(t, e) });
    }
  };

  const busy = moving !== null;
  const pending = data.pendingDataDir;

  return (
    <>
      <div>
        <SettingRow label={t('storage.models')}>
          <Button
            size="sm"
            disabled={busy}
            onClick={() => void pickThen(t('storage.pickModels'), data.modelsDir, moveModels)}
          >
            {t('common.change')}
          </Button>
          {data.modelsDefault ? null : (
            <Button size="sm" variant="quiet" disabled={busy} onClick={() => void moveModels(null)}>
              {t('storage.useDefault')}
            </Button>
          )}
        </SettingRow>
        <p className="st-desc st-path">{data.modelsDir}</p>
        {moving === 'models' ? <ProgressBar value={null} label={t('storage.moving')} /> : null}
      </div>
      <div>
        <SettingRow label={t('storage.data')}>
          <Button
            size="sm"
            disabled={busy}
            onClick={() => void pickThen(t('storage.pickData'), pending ?? data.dataDir, moveData)}
          >
            {t('common.change')}
          </Button>
          {!data.dataDefault || pending ? (
            <Button size="sm" variant="quiet" disabled={busy} onClick={() => void moveData(null)}>
              {t('storage.useDefault')}
            </Button>
          ) : null}
          {mode === 'settings' && pending ? (
            <Button size="sm" variant="primary" disabled={busy} onClick={() => void restart()}>
              {t('storage.restartNow')}
            </Button>
          ) : null}
        </SettingRow>
        <p className="st-desc st-path">{data.dataDir}</p>
        {pending ? (
          <p className="st-desc">
            {t(mode === 'settings' ? 'storage.dataPending' : 'storage.dataPendingSetup', { path: pending })}
          </p>
        ) : null}
        {data.moveError ? (
          <p className="st-error" role="alert">
            {t('storage.dataMoveFailed', { reason: errorMessage(t, data.moveError) })}
          </p>
        ) : null}
      </div>
      {confirming ? (
        <Dialog
          title={t('storage.moveDataTitle')}
          onClose={() => void cancelMove()}
          actions={
            <>
              <Button variant="quiet" onClick={() => void cancelMove()}>{t('common.cancel')}</Button>
              <Button variant="primary" autoFocus onClick={() => void restart()}>
                {t('storage.moveAndRestart')}
              </Button>
            </>
          }
        >
          <p className="st-body">{t('storage.moveDataBody', { path: confirming })}</p>
        </Dialog>
      ) : null}
    </>
  );
}
