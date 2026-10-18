// The five global hotkeys: Kbd + Change. Recording captures the next key combo (plan Step 12).
// Shared by Settings › General and onboarding step 4.
import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { Button, Kbd } from '../../ds';
import { errorText } from '../../shared/errors';
import { useT, type I18nKey } from '../../shared/i18n';
import { setHotkey, suspendHotkeys } from '../../shared/ipc';
import { SETTINGS_KEY, useSetting } from '../../shared/queries';
import type { HotkeyAction, Json } from '../../shared/types';
import { acceleratorFromEvent, acceleratorKeys } from './accelerator';

const ACTIONS: readonly { action: HotkeyAction; label: I18nKey }[] = [
  { action: 'focusPet', label: 'settings.hotkeys.focusPet' },
  { action: 'sendSelection', label: 'settings.hotkeys.sendSelection' },
  { action: 'captureScreen', label: 'settings.hotkeys.captureScreen' },
  { action: 'quickNote', label: 'settings.hotkeys.quickNote' },
  { action: 'recordMeeting', label: 'settings.hotkeys.recordMeeting' },
];

export function HotkeyRows() {
  const t = useT();
  const qc = useQueryClient();
  const hotkeys = useSetting<Record<string, Json>>('hotkeys', {});
  const [recording, setRecording] = useState<HotkeyAction | null>(null);
  const [errors, setErrors] = useState<Partial<Record<HotkeyAction, string>>>({});

  useEffect(() => {
    if (!recording) return;
    const action = recording;
    const setError = (msg: string | null) =>
      setErrors((prev) => ({ ...prev, [action]: msg ?? undefined }));

    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.code === 'Escape' && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
        setRecording(null);
        return;
      }
      const r = acceleratorFromEvent(e);
      if (r.kind === 'wait') return;
      setRecording(null);
      if (r.kind === 'invalid') {
        setError(t('settings.hotkeys.invalid'));
        return;
      }
      setHotkey(action, r.accelerator)
        .then(() => setError(null))
        .catch((err: unknown) => {
          const code = errorText(err);
          setError(
            code === 'inUse'
              ? t('settings.hotkeys.inUse')
              : code === 'invalid'
                ? t('settings.hotkeys.invalid')
                : t('settings.hotkeys.failed', { reason: code }),
          );
        })
        .finally(() => void qc.invalidateQueries({ queryKey: SETTINGS_KEY }));
    };
    const cancel = () => setRecording(null);
    // Registered hotkeys would swallow their keys (and fire their action) before this page sees
    // them; suspended, pressing another action's shortcut reaches `set_hotkey` and reports inUse.
    void suspendHotkeys(true);
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('blur', cancel);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('blur', cancel);
      void suspendHotkeys(false);
    };
  }, [recording, qc, t]);

  return (
    <ul className="st-hk">
      {ACTIONS.map(({ action, label }) => {
        const acc = hotkeys[action];
        const isRec = recording === action;
        const errId = `hk-err-${action}`;
        return (
          <li key={action} className="st-hk__row">
            <span className="st-hk__label">{t(label)}</span>
            <span className="st-hk__keys" aria-live="polite">
              {isRec ? (
                <span className="st-hk__rec">{t('settings.hotkeys.recording')}</span>
              ) : typeof acc === 'string' && acc ? (
                <Kbd keys={acceleratorKeys(acc, t('settings.hotkeys.space'))} />
              ) : (
                <span className="st-hk__muted">{t('settings.hotkeys.notSet')}</span>
              )}
            </span>
            <Button
              size="sm"
              variant={isRec ? 'quiet' : 'secondary'}
              aria-describedby={errors[action] ? errId : undefined}
              aria-label={isRec ? undefined : `${t('common.change')} — ${t(label)}`}
              onClick={() => {
                setErrors((prev) => ({ ...prev, [action]: undefined }));
                setRecording(isRec ? null : action);
              }}
            >
              {isRec ? t('common.cancel') : t('common.change')}
            </Button>
            {errors[action] ? (
              <p className="st-error st-hk__err" id={errId} role="alert">{errors[action]}</p>
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}
