// Integrations › Cloud providers: one provider for all background AI (local model, a key provider
// or an Ollama / LM Studio server), its key or address, model, Test connection and the meetings opt-in.
import { useQuery } from '@tanstack/react-query';
import { useId, useState, type FormEvent, type ReactElement } from 'react';
import { Button, Icon, StatusPill, Switch } from '../../ds';
import { errorMessage } from '../../shared/errors';
import { useT } from '../../shared/i18n';
import { isWindows } from '../../shared/platform';
import { aiConfigure, aiRemoveKey, aiServerModels, aiSetKey, aiTest } from '../../shared/ipc';
import { useAiStatus, useSetSetting, useSetting } from '../../shared/queries';
import type { AiProvider, CloudProvider, CloudProviderStatus, KeyProvider } from '../../shared/types';
import { useAiAction } from '../settings/ModelCards';
import { SettingRow } from '../settings/parts';
import { AI_PROVIDER_NAME, KEY_PLACEHOLDER, activeCloud, isKeyProvider, testErrorText } from './cloudModel';

const PROVIDERS = Object.keys(AI_PROVIDER_NAME) as CloudProvider[];

type TestState = { state: 'testing' } | { state: 'ok' } | { state: 'failed'; error: unknown } | null;

export function CloudProvidersCard(): ReactElement {
  const t = useT();
  const run = useAiAction();
  const selectId = useId();
  const { data: status } = useAiStatus();
  const localOnly = useSetting<boolean>('privacy.localOnly', false);
  const provider: AiProvider = status?.cloud.provider ?? 'local';
  const active = activeCloud(status);

  let pill: ReactElement;
  if (!active) pill = <StatusPill status="local" label={t('common.localOnly')} />;
  else if (active.ready) pill = <StatusPill status="synced" label={t('settings.triage.inUse')} />;
  else pill = <StatusPill status="pending" label={t('integrations.ai.notSetUp')} />;

  return (
    <article className="in-card" aria-label={t('integrations.cloud.title')}>
      <div className="in-row">
        <Icon name="link" size={20} />
        <div className="in-row__text">
          <b className="in-row__name">{t('integrations.cloud.title')}</b>
          {t('integrations.cloud.desc')}
        </div>
        {pill}
      </div>
      <SettingRow label={t('integrations.ai.provider')} htmlFor={selectId}>
        <select
          id={selectId}
          className="st-input in-select"
          value={provider}
          onChange={(e) => void run(() => aiConfigure(e.currentTarget.value as AiProvider))}
        >
          <option value="local">{t('integrations.ai.local')}</option>
          {PROVIDERS.map((p) => (
            <option key={p} value={p} disabled={localOnly}>{AI_PROVIDER_NAME[p]}</option>
          ))}
        </select>
      </SettingRow>
      {localOnly ? <p className="caption in-muted in-m0">{t('integrations.ai.localOnly')}</p> : null}
      {active ? <ProviderSetup key={active.id} p={active} /> : null}
    </article>
  );
}

/** Key or address, model, Test connection, meetings switch and privacy note for one cloud provider. */
function ProviderSetup({ p }: { p: CloudProviderStatus }): ReactElement {
  const t = useT();
  const run = useAiAction();
  const setSetting = useSetSetting();
  const meetings = useSetting<boolean>('ai.cloudMeetings', false);
  const [test, setTest] = useState<TestState>(null);
  const name = AI_PROVIDER_NAME[p.id];

  const runTest = () =>
    run(async () => {
      setTest({ state: 'testing' });
      try {
        await aiTest(p.id);
        setTest({ state: 'ok' });
      } catch (e) {
        setTest({ state: 'failed', error: e });
      }
    });

  return (
    <>
      {isKeyProvider(p.id) ? (
        <KeySetup p={p} keyProvider={p.id} onTest={runTest} onChange={() => setTest(null)} />
      ) : (
        <ServerSetup p={p} server={p.id as 'ollama' | 'lmstudio'} onTest={runTest} onChange={() => setTest(null)} />
      )}
      {test?.state === 'testing' ? <p className="caption in-muted in-m0" role="status">{t('integrations.ai.testing')}</p> : null}
      {test?.state === 'ok' ? (
        <div className="in-actions" role="status">
          <StatusPill status="synced" label={t('integrations.ai.ok')} />
        </div>
      ) : null}
      {test?.state === 'failed' ? (
        <p className="st-error in-m0" role="alert">{testErrorText(t, test.error)}</p>
      ) : null}
      <Switch
        id="in-ai-meetings"
        label={t('integrations.ai.meetings', { provider: name })}
        description={t('integrations.ai.meetingsDesc')}
        checked={meetings}
        onChange={(v) => void setSetting('ai.cloudMeetings', v)}
      />
      <p className="caption in-muted in-m0">{t('integrations.ai.sends', { provider: name })}</p>
    </>
  );
}

interface SetupProps {
  p: CloudProviderStatus;
  onTest: () => Promise<void>;
  onChange: () => void;
}

function KeySetup({ p, keyProvider, onTest, onChange }: SetupProps & { keyProvider: KeyProvider }): ReactElement {
  const t = useT();
  const run = useAiAction();
  const modelId = useId();
  const [key, setKey] = useState('');

  const save = async (e: FormEvent) => {
    e.preventDefault();
    const value = key.trim();
    if (!value) return;
    let saved = false;
    await run(async () => {
      await aiSetKey(keyProvider, value);
      saved = true;
    });
    if (saved) {
      setKey('');
      await onTest();
    }
  };

  return (
    <>
      {p.hasKey ? (
        <div className="in-actions">
          <StatusPill status="local" icon="lock" label={t('integrations.ai.keySaved')} />
          <Button variant="secondary" size="sm" onClick={() => void onTest()}>
            {t('integrations.ai.test')}
          </Button>
          <Button
            variant="quiet"
            size="sm"
            onClick={() => {
              onChange();
              void run(() => aiRemoveKey(keyProvider));
            }}
          >
            {t('integrations.ai.removeKey')}
          </Button>
        </div>
      ) : (
        <form className="in-token" onSubmit={(e) => void save(e)}>
          <div className="in-actions">
            <input
              type="password"
              className="st-input in-token__input"
              aria-label={t('integrations.ai.key')}
              placeholder={KEY_PLACEHOLDER[keyProvider]}
              autoComplete="off"
              spellCheck={false}
              value={key}
              onChange={(e) => setKey(e.currentTarget.value)}
            />
            <Button type="submit" variant="secondary" size="sm" disabled={!key.trim()}>
              {t('integrations.ai.saveKey')}
            </Button>
          </div>
          <p className="caption in-muted in-m0">
            {t(isWindows ? 'integrations.ai.keyHelpWin' : 'integrations.ai.keyHelp', { provider: AI_PROVIDER_NAME[keyProvider] })}
          </p>
        </form>
      )}
      <SettingRow label={t('integrations.ai.model')} htmlFor={modelId}>
        <select
          id={modelId}
          className="st-input in-select"
          value={p.model ?? ''}
          onChange={(e) => {
            onChange();
            void run(() => aiConfigure(p.id, e.currentTarget.value));
          }}
        >
          {p.model ? null : <option value="">{t('integrations.ai.pickModel')}</option>}
          {p.models.map((m) => (
            <option key={m.id} value={m.id}>{m.label}</option>
          ))}
        </select>
      </SettingRow>
    </>
  );
}

function ServerSetup({ p, server, onTest, onChange }: SetupProps & { server: 'ollama' | 'lmstudio' }): ReactElement {
  const t = useT();
  const run = useAiAction();
  const urlId = useId();
  const modelId = useId();
  const [url, setUrl] = useState(p.baseUrl ?? '');
  const models = useQuery({
    queryKey: ['aiServerModels', server, p.baseUrl],
    queryFn: () => aiServerModels(server),
    retry: false,
    enabled: p.baseUrl != null,
  });
  const list = models.data ?? [];
  // Keep a saved model visible even when the server doesn't list it (yet).
  const options = p.model && !list.includes(p.model) ? [p.model, ...list] : list;

  return (
    <>
      <SettingRow label={t('integrations.ai.serverUrl')} htmlFor={urlId}>
        <div className="in-actions">
          <input
            id={urlId}
            className="st-input in-token__input"
            value={url}
            autoComplete="off"
            spellCheck={false}
            onChange={(e) => setUrl(e.currentTarget.value)}
          />
          <Button
            variant="secondary"
            size="sm"
            disabled={!url.trim()}
            onClick={() => {
              onChange();
              void run(() => aiConfigure(p.id, null, url.trim()));
            }}
          >
            {t('integrations.ai.saveUrl')}
          </Button>
        </div>
      </SettingRow>
      <SettingRow label={t('integrations.ai.model')} htmlFor={modelId}>
        <div className="in-actions">
          <select
            id={modelId}
            className="st-input in-select"
            value={p.model ?? ''}
            onChange={(e) => {
              onChange();
              void run(() => aiConfigure(p.id, e.currentTarget.value));
            }}
          >
            {p.model ? null : <option value="">{t('integrations.ai.pickModel')}</option>}
            {options.map((m) => (
              <option key={m} value={m}>{m}</option>
            ))}
          </select>
          <Button variant="quiet" size="sm" onClick={() => void models.refetch()}>
            {t('integrations.ai.refreshModels')}
          </Button>
        </div>
      </SettingRow>
      {models.isError ? <p className="st-error in-m0">{errorMessage(t, models.error)}</p> : null}
      {models.isSuccess && !list.length ? (
        <p className="caption in-muted in-m0">{t('integrations.ai.noServerModels')}</p>
      ) : null}
      <div className="in-actions">
        <Button variant="secondary" size="sm" onClick={() => void onTest()}>
          {t('integrations.ai.test')}
        </Button>
      </div>
    </>
  );
}
