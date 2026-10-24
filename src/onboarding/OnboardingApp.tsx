// Onboarding window (plan Step 13): Your pet · Permissions · Local AI · Apps · Hotkeys.
// Windows skips Permissions: it has no consent prompts for screen capture or accessibility.
import { Fragment, useEffect, useRef, useState } from 'react';
import { Button } from '../ds';
import { useAiAction } from '../home/settings/ModelCards';
import { WindowControls } from '../home/WindowControls';
import { asSpecies, resolveSkin, type Species } from '../pet/catalog';
import { errorText } from '../shared/errors';
import { LANG_LABEL, LANGS, useLang, useT, type I18nKey } from '../shared/i18n';
import { isWindows } from '../shared/platform';
import { aiDownload, aiSelectModel, finishOnboarding } from '../shared/ipc';
import { useAiStatus, useSetSetting, useSetting } from '../shared/queries';
import { AiStep, AppsStep, DownloadStrip, HotkeysStep, PermissionsStep, PetStep } from './steps';
import './onboarding.css';

type StepId = 'pet' | 'permissions' | 'ai' | 'apps' | 'hotkeys';

const ALL_STEPS: readonly { id: StepId; label: I18nKey; note: I18nKey }[] = [
  { id: 'pet', label: 'onboarding.step.pet', note: 'onboarding.note.pet' },
  { id: 'permissions', label: 'onboarding.step.permissions', note: 'onboarding.note.permissions' },
  { id: 'ai', label: 'onboarding.step.ai', note: 'onboarding.note.ai' },
  { id: 'apps', label: 'onboarding.step.apps', note: 'onboarding.note.apps' },
  { id: 'hotkeys', label: 'onboarding.step.hotkeys', note: 'onboarding.note.hotkeys' },
];
const STEPS = isWindows ? ALL_STEPS.filter((s) => s.id !== 'permissions') : ALL_STEPS;
const LAST = STEPS.length - 1;
// Windows starts the model download after the Local AI step, so it lands in the models folder picked there.
const DOWNLOAD_AFTER: StepId = isWindows ? 'ai' : 'pet';

export function OnboardingApp() {
  const t = useT();
  const lang = useLang();
  const set = useSetSetting();
  const runAi = useAiAction();
  const { data: ai } = useAiStatus();
  const savedName = useSetting<string>('pet.name', 'Miso');
  const savedLook = asSpecies(useSetting<string>('pet.style', 'pixel'));
  const savedSkin = useSetting<string>('pet.skin', 'peach');
  const localOnly = useSetting<boolean>('privacy.localOnly', false);
  const activeModel = useSetting<string | null>('ai.modelId', null);

  const [step, setStep] = useState(0);
  const [name, setName] = useState<string | null>(null);
  const [look, setLook] = useState<Species | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mainRef = useRef<HTMLElement>(null);

  const petLook = look ?? savedLook;
  const skin = resolveSkin(petLook, savedSkin);
  const current = STEPS[step].id;

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
      if (current === 'pet') {
        await set('pet.name', (name ?? savedName).trim() || 'Miso');
        await set('pet.style', petLook);
      }
      // Start the chosen (else recommended) model once. A model already on disk (Delete everything
      // keeps models/) only needs selecting.
      if (current === DOWNLOAD_AFTER && !localOnly && ai && ai.state === 'none') {
        const wanted = ai.modelId ?? ai.recommendedId;
        const onDisk = ai.models.find((m) => m.id === wanted && m.downloaded)
          ?? (ai.modelId ? undefined : ai.models.find((m) => m.downloaded));
        if (!onDisk) await runAi(() => aiDownload(wanted));
        else if (!activeModel) await runAi(() => aiSelectModel(onDisk.id));
      }
      if (step === LAST) {
        // Rust closes this window, creates the pet and applies autostart; with a data move pending it
        // restarts first and does that on the next start.
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
        <select
          className="ob-lang"
          aria-label={t('settings.language')}
          value={lang}
          onChange={(e) => void set('ui.language', e.currentTarget.value)}
        >
          {LANGS.map((l) => (
            <option key={l} value={l}>
              {t(LANG_LABEL[l])}
            </option>
          ))}
        </select>
        {isWindows && <WindowControls maximizable={false} />}
      </header>

      <main ref={mainRef} tabIndex={-1} style={{ flex: '1 1 0', minHeight: 0, display: 'flex', outline: 'none' }}>
        {current === 'pet' ? (
          <PetStep name={name ?? savedName} onName={setName} look={petLook} onLook={setLook} skin={skin} />
        ) : current === 'permissions' ? (
          <PermissionsStep look={petLook} skin={skin} />
        ) : current === 'ai' ? (
          <AiStep look={petLook} skin={skin} status={ai} localOnly={localOnly} />
        ) : current === 'apps' ? (
          <AppsStep look={petLook} skin={skin} />
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
