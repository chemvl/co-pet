// Onboarding window (plan Step 13): Your pet · Permissions · Local AI · Hotkeys.
import { Fragment, useEffect, useRef, useState } from 'react';
import { Button, SegmentedControl } from '../ds';
import { useAiAction } from '../home/settings/ModelCards';
import { asLook, asSkin, errorText } from '../home/settings/parts';
import type { CatLook } from '../pet/CatArt';
import { useLang, useT, type I18nKey } from '../shared/i18n';
import { aiDownload, aiSelectModel, finishOnboarding } from '../shared/ipc';
import { useAiStatus, useSetSetting, useSetting } from '../shared/queries';
import { AiStep, DownloadStrip, HotkeysStep, PermissionsStep, PetStep } from './steps';
import './onboarding.css';

const STEPS: readonly { label: I18nKey; note: I18nKey }[] = [
  { label: 'onboarding.step.pet', note: 'onboarding.note.pet' },
  { label: 'onboarding.step.permissions', note: 'onboarding.note.permissions' },
  { label: 'onboarding.step.ai', note: 'onboarding.note.ai' },
  { label: 'onboarding.step.hotkeys', note: 'onboarding.note.hotkeys' },
];
const LAST = STEPS.length - 1;

export function OnboardingApp() {
  const t = useT();
  const lang = useLang();
  const set = useSetSetting();
  const runAi = useAiAction();
  const { data: ai } = useAiStatus();
  const savedName = useSetting<string>('pet.name', 'Miso');
  const savedLook = asLook(useSetting<string>('pet.style', 'soft'));
  const skin = asSkin(useSetting<string>('pet.skin', 'peach'));
  const localOnly = useSetting<boolean>('privacy.localOnly', false);
  const activeModel = useSetting<string | null>('ai.modelId', null);

  const [step, setStep] = useState(0);
  const [name, setName] = useState<string | null>(null);
  const [look, setLook] = useState<CatLook | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mainRef = useRef<HTMLElement>(null);

  const petLook = look ?? savedLook;

  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);
  useEffect(() => {
    mainRef.current?.focus();
  }, [step]);

  const next = async () => {
    setError(null);
    setBusy(true);
    try {
      if (step === 0) {
        await set('pet.name', (name ?? savedName).trim() || 'Miso');
        await set('pet.style', petLook);
        // Start the recommended model as early as possible, once. A model already on disk
        // (Delete everything keeps models/) only needs selecting.
        if (!localOnly && ai && ai.state === 'none') {
          const onDisk =
            ai.models.find((m) => m.id === ai.recommendedId && m.downloaded) ??
            ai.models.find((m) => m.downloaded);
          if (!onDisk) await runAi(() => aiDownload(ai.recommendedId));
          else if (!activeModel) await runAi(() => aiSelectModel(onDisk.id));
        }
      }
      if (step === LAST) {
        // Rust closes this window, creates the pet and applies autostart.
        await finishOnboarding();
        return;
      }
      setStep(step + 1);
    } catch (e) {
      setError(t('onboarding.finishFailed', { reason: errorText(e) }));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="ob-win">
      <header className="ob-top" data-tauri-drag-region>
        <ol className="ob-steps" aria-label={t('onboarding.steps')} data-tauri-drag-region>
          {STEPS.map((s, i) => (
            <Fragment key={s.label}>
              {i > 0 ? <li className="ob-sep" aria-hidden="true" /> : null}
              <li
                className={i === step ? 'ob-st is-on' : i < step ? 'ob-st is-done' : 'ob-st'}
                aria-current={i === step ? 'step' : undefined}
              >
                <i aria-hidden="true">{i + 1}</i>
                {t(s.label)}
              </li>
            </Fragment>
          ))}
        </ol>
        <SegmentedControl
          label={t('settings.language')}
          value={lang}
          onChange={(v) => void set('ui.language', v)}
          options={[
            { value: 'en', label: t('common.english') },
            { value: 'vi', label: t('common.vietnamese') },
          ]}
        />
      </header>

      <main ref={mainRef} tabIndex={-1} style={{ flex: '1 1 0', minHeight: 0, display: 'flex', outline: 'none' }}>
        {step === 0 ? (
          <PetStep name={name ?? savedName} onName={setName} look={petLook} onLook={setLook} skin={skin} />
        ) : step === 1 ? (
          <PermissionsStep look={petLook} skin={skin} />
        ) : step === 2 ? (
          <AiStep look={petLook} skin={skin} status={ai} localOnly={localOnly} />
        ) : (
          <HotkeysStep look={petLook} skin={skin} />
        )}
      </main>

      {step > 0 && ai && !localOnly ? <DownloadStrip status={ai} /> : null}

      <footer className="ob-ft">
        <span className="ob-ft__note">
          {error ? (
            <span className="st-error" role="alert">{error}</span>
          ) : (
            `${t('onboarding.progress', { n: step + 1, total: STEPS.length })} · ${t(STEPS[step].note)}`
          )}
        </span>
        <Button variant="quiet" disabled={step === 0 || busy} onClick={() => setStep(step - 1)}>
          {t('onboarding.back')}
        </Button>
        <Button variant="primary" disabled={busy} onClick={() => void next()}>
          {step === LAST ? t('onboarding.finish') : t('onboarding.continue')}
        </Button>
      </footer>
    </div>
  );
}
