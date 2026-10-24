// Pieces shared by the Notion and Asana cards: the card shell, the connect / reconnect block
// (OAuth through the backend or a pasted token), Disconnect, and the queue line.
import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useId, useState, type ReactElement, type ReactNode } from 'react';
import { Button, Icon, StatusPill } from '../../ds';
import { errorMessage } from '../../shared/errors';
import { useT, type I18nKey } from '../../shared/i18n';
import { connectOauth, connectToken, disconnect, on, retryExports } from '../../shared/ipc';
import { INTEGRATIONS_KEY, useBackendStatus } from '../../shared/queries';
import { useToast } from '../../shared/Toast';
import type { ExportProvider, ProviderStatus } from '../../shared/types';
import { PROVIDER_NAME, queueLine } from './model';
import '../settings/settings.css';
import './integrations.css';

/** Refreshes integration state (and task export pills) and toasts failures. */
export function useIntegrationRun(): (op: () => Promise<unknown>) => Promise<boolean> {
  const t = useT();
  const qc = useQueryClient();
  const { show } = useToast();
  return async (op) => {
    try {
      await op();
      return true;
    } catch (e) {
      show({ message: errorMessage(t, e) });
      return false;
    } finally {
      void qc.invalidateQueries({ queryKey: INTEGRATIONS_KEY });
    }
  };
}

export function ProviderCard(p: {
  provider: ExportProvider;
  caption: string;
  pill?: ReactNode;
  children?: ReactNode;
}): ReactElement {
  const name = PROVIDER_NAME[p.provider];
  return (
    <article className="in-card" aria-label={name} data-provider={p.provider}>
      <div className="in-card__head">
        <span className="in-logo" aria-hidden="true">{name[0]}</span>
        <div className="in-card__title">
          <b>{name}</b>
          <div className="caption in-muted">{p.caption}</div>
        </div>
        {p.pill}
      </div>
      {p.children}
    </article>
  );
}

const COPY: Record<ExportProvider, { connect: I18nKey; tokenLabel: I18nKey; tokenHelp: I18nKey; placeholder: I18nKey }> = {
  notion: {
    connect: 'integrations.notion.connect',
    tokenLabel: 'integrations.notion.tokenLabel',
    tokenHelp: 'integrations.notion.tokenHelp',
    placeholder: 'integrations.notion.tokenPlaceholder',
  },
  asana: {
    connect: 'integrations.asana.connect',
    tokenLabel: 'integrations.asana.tokenLabel',
    tokenHelp: 'integrations.asana.tokenHelp',
    placeholder: 'integrations.asana.tokenPlaceholder',
  },
};

/**
 * Not connected (or needs a reconnect): "Connect <Provider>" through the browser once the user
 * connected their own Supabase project (Settings › Account), and "Use a token instead". The
 * OAuth result arrives as `integration-changed`.
 * `compact` (onboarding) keeps the footer's Continue the only primary button.
 */
export function ConnectSection(p: { provider: ExportProvider; reconnect?: boolean; compact?: boolean }): ReactElement {
  const t = useT();
  const qc = useQueryClient();
  const tokenId = useId();
  const { data: backend } = useBackendStatus();
  const oauth = backend?.configured === true;
  const copy = COPY[p.provider];
  const [tokenOpen, setTokenOpen] = useState(false);
  const [token, setToken] = useState('');
  const [pending, setPending] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Without a backend the token form is the only way in.
  const showToken = tokenOpen || backend?.configured === false;

  useEffect(() => {
    const sub = on('integration-changed', (e) => {
      if (e.provider !== p.provider) return;
      setPending(false);
      setError(e.ok ? null : errorMessage(t, e.error ?? 'invalid'));
    });
    return () => void sub.then((unlisten) => unlisten());
  }, [p.provider, t]);

  const startOauth = async () => {
    setError(null);
    try {
      await connectOauth(p.provider);
      setPending(true);
    } catch (e) {
      setError(errorMessage(t, e));
    }
  };

  const submitToken = async () => {
    setError(null);
    setBusy(true);
    try {
      await connectToken(p.provider, token.trim());
      setToken('');
      setTokenOpen(false);
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setBusy(false);
      void qc.invalidateQueries({ queryKey: INTEGRATIONS_KEY });
    }
  };

  const connectLabel = p.reconnect ? t('integrations.reconnect') : t(copy.connect);
  return (
    <div className="in-connect">
      {pending ? <p className="caption in-muted in-m0" role="status">{t('integrations.pending')}</p> : null}
      <div className="in-actions">
        {oauth ? (
          <Button variant={p.compact ? 'secondary' : 'primary'} size="sm" onClick={() => void startOauth()}>
            {connectLabel}
          </Button>
        ) : null}
        {oauth && !tokenOpen ? (
          <Button variant="quiet" size="sm" onClick={() => setTokenOpen(true)}>
            {t('integrations.useToken')}
          </Button>
        ) : null}
      </div>
      {showToken ? (
        <form
          className="in-token"
          onSubmit={(e) => {
            e.preventDefault();
            if (token.trim() && !busy) void submitToken();
          }}
        >
          <div className="in-actions">
            <input
              id={tokenId}
              type="password"
              className="st-input in-token__input"
              placeholder={t(copy.placeholder)}
              aria-label={t(copy.tokenLabel)}
              autoComplete="off"
              spellCheck={false}
              value={token}
              onChange={(e) => setToken(e.target.value)}
            />
            <Button type="submit" variant="secondary" size="sm" disabled={!token.trim() || busy}>
              {p.reconnect && !oauth ? connectLabel : t('integrations.connect')}
            </Button>
          </div>
          <p className="caption in-muted in-m0">{t(copy.tokenHelp)}</p>
        </form>
      ) : null}
      {error ? <p className="st-error" role="alert">{error}</p> : null}
    </div>
  );
}

/**
 * The states every provider card shares: not connected (connect block under `caption`), needs
 * reconnect, and connected in `compact` mode (onboarding: no target pickers). Otherwise renders
 * `children` with the connected status, its account label and the "Connected" pill.
 */
export function ConnectionStates(p: {
  provider: ExportProvider;
  status: ProviderStatus | undefined;
  compact?: boolean;
  caption: string;
  children: (status: ProviderStatus, account: string, connectedPill: ReactElement) => ReactElement;
}): ReactElement {
  const t = useT();
  const { provider, status, compact } = p;
  if (!status?.connected) {
    return (
      <ProviderCard provider={provider} caption={p.caption}>
        {/* Synced from a Mac that holds the token; connecting here uses that target as-is. */}
        {status?.target ? (
          <p className="caption in-muted in-m0">{t('integrations.setUpElsewhere', { target: status.target.targetLabel })}</p>
        ) : null}
        <ConnectSection provider={provider} compact={compact} />
      </ProviderCard>
    );
  }
  const account = status.accountLabel ?? '';
  if (status.needsReconnect) {
    return (
      <ProviderCard
        provider={provider}
        caption={t('integrations.connectedAs', { account })}
        pill={<StatusPill status="failed" label={t('integrations.reconnectNeeded')} />}
      >
        <ConnectSection provider={provider} reconnect compact={compact} />
        {compact ? null : <QueueLine status={status} />}
      </ProviderCard>
    );
  }
  const connectedPill = <StatusPill status="synced" label={t('integrations.connected')} />;
  if (compact) {
    return <ProviderCard provider={provider} caption={t('integrations.connectedAs', { account })} pill={connectedPill} />;
  }
  return p.children(status, account, connectedPill);
}

export function DisconnectButton({ provider }: { provider: ExportProvider }): ReactElement {
  const t = useT();
  const run = useIntegrationRun();
  return (
    <Button variant="quiet" size="sm" onClick={() => void run(() => disconnect(provider))}>
      {t('integrations.disconnect')}
    </Button>
  );
}

/** "{n} export waiting · {reason}" + Retry now, while this device has queued or failed jobs. */
export function QueueLine({ status }: { status: ProviderStatus }): ReactElement | null {
  const t = useT();
  const run = useIntegrationRun();
  const line = queueLine(status, t);
  if (!line) return null;
  return (
    <div className="in-queue">
      <Icon name="sync" size={16} />
      <span className="in-queue__text">{line}</span>
      <Button variant="secondary" size="sm" onClick={() => void run(() => retryExports(status.provider))}>
        {t('integrations.retryNow')}
      </Button>
    </div>
  );
}
