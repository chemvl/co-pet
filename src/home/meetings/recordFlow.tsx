// Home's Record meeting, Stop and Import audio or video, shared by the Meetings page and the
// palette. The first recording asks for consent in a dialog (the pet's consent copy), like the pet.
import { open } from '@tauri-apps/plugin-dialog';
import { createContext, useContext, useState, type ReactElement, type ReactNode } from 'react';
import { Button } from '../../ds';
import { errorMessage, errorText } from '../../shared/errors';
import { useT } from '../../shared/i18n';
import { importMeeting, requestPermission, startRecording, stopRecording } from '../../shared/ipc';
import { useRecording, useSetSetting, useSetting } from '../../shared/queries';
import { useToast } from '../../shared/Toast';
import { navigate } from '../router';
import { Dialog } from '../settings/parts';
import './meetings.css';

const IMPORT_EXTENSIONS = ['m4a', 'mp3', 'wav', 'aac', 'flac', 'ogg', 'aiff', 'caf', 'mp4', 'mov', 'm4v'];

export interface MeetingActions {
  /** Starts a recording (after the consent dialog the first time); no-op while one runs. */
  record(): void;
  stop(): void;
  importFile(): void;
  /** True while the recorder records or is paused. */
  recording: boolean;
  /** A start or stop is in flight. */
  busy: boolean;
}

const MeetingActionsContext = createContext<MeetingActions | null>(null);

export function useMeetingActions(): MeetingActions {
  const actions = useContext(MeetingActionsContext);
  if (!actions) throw new Error('useMeetingActions() requires <MeetingActionsProvider>');
  return actions;
}

const openMeeting = (id: string) => navigate(`meetings/${encodeURIComponent(id)}`);

export function MeetingActionsProvider({ children }: { children: ReactNode }): ReactElement {
  const t = useT();
  const { show } = useToast();
  const setSetting = useSetSetting();
  const consentShown = useSetting<boolean>('transcribe.consentShown', false);
  const recording = (useRecording().data?.state ?? 'idle') !== 'idle';
  const [consent, setConsent] = useState(false);
  const [busy, setBusy] = useState(false);

  const permissionAction = (kind: 'microphone' | 'screenRecording') => ({
    label: t('onboarding.perm.open'),
    run: () => void requestPermission(kind).catch((e: unknown) => show({ message: errorMessage(t, e) })),
  });

  const startNow = () => {
    setBusy(true);
    startRecording()
      .then(
        (s) => {
          if (!s.mic) show({ message: t('notice.recMicMissing'), action: permissionAction('microphone') });
          else if (!s.system) show({ message: t('notice.recSystemMissing'), action: permissionAction('screenRecording') });
          if (s.meetingId) openMeeting(s.meetingId);
        },
        (e: unknown) =>
          show({
            message: errorMessage(t, e),
            action: errorText(e) === 'noAudioSource' ? permissionAction('microphone') : undefined,
          }),
      )
      .finally(() => setBusy(false));
  };

  const actions: MeetingActions = {
    record() {
      if (recording || busy) return;
      if (consentShown) startNow();
      else setConsent(true);
    },
    stop() {
      setBusy(true);
      stopRecording()
        .then(openMeeting, (e: unknown) => show({ message: errorMessage(t, e) }))
        .finally(() => setBusy(false));
    },
    importFile() {
      void (async () => {
        try {
          const path = await open({
            multiple: false,
            directory: false,
            filters: [{ name: t('meetings.importFilter'), extensions: IMPORT_EXTENSIONS }],
          });
          if (typeof path !== 'string') return;
          openMeeting(await importMeeting(path));
        } catch (e) {
          show({ message: errorMessage(t, e) });
        }
      })();
    },
    recording,
    busy,
  };

  const onConsentStart = () => {
    setConsent(false);
    void setSetting('transcribe.consentShown', true);
    startNow();
  };

  return (
    <MeetingActionsContext.Provider value={actions}>
      {children}
      {consent ? (
        <Dialog
          title={t('rec.consent.title')}
          onClose={() => setConsent(false)}
          actions={
            <>
              <Button variant="quiet" onClick={() => setConsent(false)}>{t('common.cancel')}</Button>
              <Button variant="primary" icon="mic" autoFocus onClick={onConsentStart}>{t('rec.consent.start')}</Button>
            </>
          }
        >
          <p className="body mt-m0">{t('rec.consent.body')}</p>
        </Dialog>
      ) : null}
    </MeetingActionsContext.Provider>
  );
}
