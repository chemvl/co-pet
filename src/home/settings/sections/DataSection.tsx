import { open } from '@tauri-apps/plugin-dialog';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { useEffect, useId, useRef, useState } from 'react';
import { Button } from '../../../ds';
import { errorText } from '../../../shared/errors';
import { useT } from '../../../shared/i18n';
import { deleteEverything, exportAll } from '../../../shared/ipc';
import { useToast } from '../../../shared/Toast';
import { Group, SettingRow } from '../parts';

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
      {confirming ? <DeleteDialog onClose={() => setConfirming(false)} /> : null}
    </>
  );
}

/** Folds the two Vietnamese tone placements ("xoá" / "xóa") and case before comparing. */
function normalize(s: string): string {
  return s.normalize('NFC').trim().toLowerCase().replace(/\s+/g, ' ').replace(/oá/g, 'óa');
}

function DeleteDialog({ onClose }: { onClose: () => void }) {
  const t = useT();
  const titleId = useId();
  const descId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const phrase = t('settings.data.deletePhrase');
  const matches = normalize(text) === normalize(phrase);

  useEffect(() => {
    inputRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const confirm = async () => {
    if (!matches || busy) return;
    setBusy(true);
    setError(null);
    try {
      // Rust stops the worker and the model, deletes the library and restarts the app.
      await deleteEverything(phrase);
    } catch (e) {
      setError(t('settings.data.deleteFailed', { reason: errorText(e) }));
      setBusy(false);
    }
  };

  return (
    <div className="hm-scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="st-dialog" role="alertdialog" aria-modal="true" aria-labelledby={titleId} aria-describedby={descId}>
        <h2 className="st-dialog__title" id={titleId}>{t('settings.data.deleteTitle')}</h2>
        <p className="st-body" id={descId}>{t('settings.data.deleteBody')}</p>
        <label className="st-row__label" htmlFor="st-delete-confirm">
          {t('settings.data.deletePrompt', { phrase })}
        </label>
        <input
          id="st-delete-confirm"
          ref={inputRef}
          className="st-input"
          value={text}
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void confirm();
          }}
        />
        {error ? <p className="st-error" role="alert">{error}</p> : null}
        <div className="st-dialog__actions">
          <Button variant="quiet" onClick={onClose}>{t('common.cancel')}</Button>
          <Button variant="danger" icon="trash" disabled={!matches || busy} onClick={() => void confirm()}>
            {t('settings.data.delete')}
          </Button>
        </div>
      </div>
    </div>
  );
}
