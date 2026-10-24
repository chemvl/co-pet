import { useQueryClient } from '@tanstack/react-query';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useEffect, useId, useState, type ReactElement } from 'react';
import { Button, StatusPill } from '../../../ds';
import { useLang, useT } from '../../../shared/i18n';
import {
  clearBackend, confirmRecoveryKey, copyConcealedText, createRecoveryKey, deleteCloudData, enterRecoveryKey, on, saveTextFile,
  sendEmailCode, setBackend, signInGoogle, signOut, syncNow, verifyEmailCode,
} from '../../../shared/ipc';
import {
  ACCOUNT_KEY, BACKEND_KEY, useAccountStatus, useBackendStatus, useSetSetting, useSetting, useSyncStatus,
} from '../../../shared/queries';
import { pendingText, syncPill } from '../../../shared/sync';
import { useToast } from '../../../shared/Toast';
import type { AccountStatus } from '../../../shared/types';
import { relativeTime } from '../../agents/activityText';
import { useNow } from '../../hooks';
import { QueryState } from '../../InboxPage';
import { Dialog, Group, SettingRow, TypedConfirmDialog, useCopied } from '../parts';
import { accountError, accountView } from './account';

const KEY_FILE_NAME = 'Co-pet recovery key.txt';
const SETUP_GUIDE_URL = 'https://mrdeeds88.github.io/copet/support/#self-host';

/** Runs one account action at a time: clears the error first, shows a failure as `error`. */
interface Run {
  busy: boolean;
  run: (op: () => Promise<unknown>) => Promise<boolean>;
  /** Shows `e` as the section's error and re-reads the status. */
  fail: (e: unknown) => void;
}

export function AccountSection({ onNavigate }: { onNavigate: (route: string) => void }): ReactElement | null {
  const t = useT();
  const qc = useQueryClient();
  const statusQuery = useAccountStatus();
  const status = statusQuery.data;
  const localOnly = useSetting<boolean>('privacy.localOnly', false);
  const [busy, setBusy] = useState(false);
  // Kept across state changes: `keyExists` is meant to be read on the recovery-key view it leads to.
  const [error, setError] = useState<string | null>(null);

  // Re-read on open and whenever Local-only mode changes what the server can be asked.
  useEffect(() => {
    void qc.invalidateQueries({ queryKey: ACCOUNT_KEY });
  }, [qc, localOnly]);

  const fail: Run['fail'] = (e) => {
    setError(accountError(t, e));
    // A refused step may mean the state moved (signed out, another Mac turned sync on).
    void qc.invalidateQueries({ queryKey: ACCOUNT_KEY });
  };

  const run: Run['run'] = async (op) => {
    setBusy(true);
    setError(null);
    try {
      await op();
      return true;
    } catch (e) {
      fail(e);
      return false;
    } finally {
      setBusy(false);
    }
  };

  if (!status) return <QueryState q={statusQuery} loading={t('settings.account.loading')} />;
  const view = accountView(status);
  const localOnlyRow = localOnly ? (
    <div className="st-row">
      <p className="st-body st-row__text">{t('settings.account.localOnly')}</p>
      <Button size="sm" onClick={() => onNavigate('settings/privacy')}>{t('settings.account.openPrivacy')}</Button>
    </div>
  ) : null;
  if (view === 'notConfigured') {
    return (
      <>
        {localOnlyRow}
        <BackendSection localOnly={localOnly} />
      </>
    );
  }

  const r: Run = { busy, run, fail };
  return (
    <>
      {localOnlyRow}
      <fieldset className="st-fieldset" disabled={localOnly}>
        {view === 'signedOut' ? <SignedOut r={r} onEventError={setError} /> : null}
        {view === 'needsSetup' ? <NeedsSetup status={status} r={r} /> : null}
        {view === 'needsRecovery' ? <NeedsRecovery status={status} r={r} /> : null}
        {view === 'ready' ? <Ready status={status} r={r} /> : null}
        {view === 'unknown' ? (
          <Group title={t('settings.account')}>
            <SignedInRow status={status} r={r} />
            <div className="st-acct__actions">
              <span className="st-body">{t('error.network')}</span>
              <Button size="sm" onClick={() => void qc.invalidateQueries({ queryKey: ACCOUNT_KEY })}>
                {t('common.retry')}
              </Button>
            </div>
          </Group>
        ) : null}
        {error ? <p className="st-error" role="alert">{error}</p> : null}
      </fieldset>
      <BackendSection localOnly={localOnly} />
    </>
  );
}

/**
 * The user's own Supabase project: a form until one is saved (Save tests it first), then its URL
 * with Change and Disconnect. Disconnect works offline; saving needs Local-only mode off.
 */
function BackendSection({ localOnly }: { localOnly: boolean }): ReactElement | null {
  const t = useT();
  const qc = useQueryClient();
  const { data: backend } = useBackendStatus();
  const [editing, setEditing] = useState(false);
  const [disconnecting, setDisconnecting] = useState(false);
  if (!backend) return null;

  const refresh = async () => {
    await Promise.all([
      qc.invalidateQueries({ queryKey: BACKEND_KEY }),
      qc.invalidateQueries({ queryKey: ACCOUNT_KEY }),
    ]);
  };

  if (!backend.configured || editing) {
    return (
      <Group
        title={t('settings.account.backendTitle')}
        desc={backend.configured ? t('settings.account.backendChangeHint') : t('settings.account.notConfigured')}
      >
        <BackendForm
          initialUrl={backend.url ?? ''}
          disabled={localOnly}
          onSaved={async () => {
            setEditing(false);
            await refresh();
          }}
          onCancel={backend.configured ? () => setEditing(false) : undefined}
        />
      </Group>
    );
  }

  return (
    <Group title={t('settings.account.backendTitle')}>
      <SettingRow label={t('settings.account.backendLabel')} desc={backend.url ?? ''}>
        <div className="st-acct__actions">
          <Button size="sm" disabled={localOnly} onClick={() => setEditing(true)}>
            {t('settings.account.backendChange')}
          </Button>
          <Button size="sm" variant="quiet" onClick={() => setDisconnecting(true)}>
            {t('settings.account.backendDisconnect')}
          </Button>
        </div>
      </SettingRow>
      {disconnecting ? <DisconnectDialog onDone={refresh} onClose={() => setDisconnecting(false)} /> : null}
    </Group>
  );
}

function BackendForm(p: {
  initialUrl: string;
  disabled: boolean;
  onSaved: () => Promise<void>;
  onCancel?: () => void;
}): ReactElement {
  const t = useT();
  const { show } = useToast();
  const urlId = useId();
  const keyId = useId();
  const [url, setUrl] = useState(p.initialUrl);
  const [key, setKey] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ready = url.trim() !== '' && key.trim() !== '' && !busy && !p.disabled;

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await setBackend(url.trim(), key.trim());
      setKey('');
      await p.onSaved();
    } catch (e) {
      setError(accountError(t, e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      className="st-acct__form"
      onSubmit={(e) => {
        e.preventDefault();
        if (ready) void save();
      }}
    >
      <fieldset className="st-fieldset" disabled={p.disabled}>
        <label className="st-row__label" htmlFor={urlId}>{t('settings.account.backendUrl')}</label>
        <input
          id={urlId}
          inputMode="url"
          className="st-input st-input--wide"
          placeholder="https://abcd.supabase.co"
          autoComplete="off"
          spellCheck={false}
          value={url}
          onChange={(e) => setUrl(e.target.value)}
        />
        <label className="st-row__label" htmlFor={keyId}>{t('settings.account.backendKey')}</label>
        <input
          id={keyId}
          className="st-input st-input--wide"
          placeholder="sb_publishable_…"
          autoComplete="off"
          spellCheck={false}
          value={key}
          onChange={(e) => setKey(e.target.value)}
        />
        <p className="st-desc">{t('settings.account.backendKeyHelp')}</p>
        <div className="st-acct__actions">
          <Button type="submit" variant="primary" size="sm" disabled={!ready}>
            {t('settings.account.backendSave')}
          </Button>
          {p.onCancel ? (
            <Button variant="quiet" size="sm" onClick={p.onCancel}>{t('common.cancel')}</Button>
          ) : null}
        </div>
      </fieldset>
      <button
        type="button"
        className="hm-link"
        onClick={() => void openUrl(SETUP_GUIDE_URL).catch((e) => show({ message: accountError(t, e) }))}
      >
        {t('settings.account.backendGuide')}
      </button>
      {error ? <p className="st-error" role="alert">{error}</p> : null}
    </form>
  );
}

function DisconnectDialog({ onDone, onClose }: { onDone: () => Promise<void>; onClose: () => void }): ReactElement {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      await clearBackend();
      await onDone();
      onClose();
    } catch (e) {
      setError(accountError(t, e));
      setBusy(false);
    }
  };
  return (
    <Dialog
      title={t('settings.account.backendDisconnectTitle')}
      role="alertdialog"
      onClose={onClose}
      actions={
        <>
          <Button variant="quiet" autoFocus onClick={onClose}>{t('common.cancel')}</Button>
          <Button variant="secondary" disabled={busy} onClick={() => void confirm()}>
            {t('settings.account.backendDisconnect')}
          </Button>
        </>
      }
    >
      <p className="st-body">{t('settings.account.backendDisconnectBody')}</p>
      {error ? <p className="st-error" role="alert">{error}</p> : null}
    </Dialog>
  );
}

function SignedOut({ r, onEventError }: { r: Run; onEventError: (error: string | null) => void }): ReactElement {
  const t = useT();
  const emailId = useId();
  const codeId = useId();
  const [email, setEmail] = useState('');
  const [sentTo, setSentTo] = useState<string | null>(null);
  const [code, setCode] = useState('');
  const [pending, setPending] = useState(false);

  // Google sign-in comes back through `copet://auth/callback`; a failure carries its code.
  useEffect(() => {
    const sub = on('account-changed', (e) => {
      setPending(false);
      onEventError(e.error ? accountError(t, e.error) : null);
    });
    return () => void sub.then((unlisten) => unlisten());
  }, [t, onEventError]);

  const sendCode = async () => {
    const to = email.trim();
    if (await r.run(() => sendEmailCode(to))) {
      setSentTo(to);
      setCode('');
    }
  };

  return (
    <Group title={t('settings.account.signedOutTitle')} desc={t('settings.account.signedOutBody')}>
      {sentTo === null ? (
        <form
          className="st-acct__form"
          onSubmit={(e) => {
            e.preventDefault();
            if (email.trim() && !r.busy) void sendCode();
          }}
        >
          <label className="st-row__label" htmlFor={emailId}>{t('settings.account.email')}</label>
          <div className="st-acct__actions">
            <input
              id={emailId}
              type="email"
              className="st-input st-input--wide"
              autoComplete="email"
              spellCheck={false}
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
            <Button type="submit" variant="primary" size="sm" disabled={!email.trim() || r.busy}>
              {t('settings.account.sendCode')}
            </Button>
          </div>
        </form>
      ) : (
        <form
          className="st-acct__form"
          onSubmit={(e) => {
            e.preventDefault();
            if (code.length === 6 && !r.busy) void r.run(() => verifyEmailCode(sentTo, code));
          }}
        >
          <p className="st-body">{t('settings.account.codeSent', { email: sentTo })}</p>
          <label className="st-row__label" htmlFor={codeId}>{t('settings.account.code')}</label>
          <div className="st-acct__actions">
            <input
              id={codeId}
              className="st-input st-input--code"
              inputMode="numeric"
              autoComplete="one-time-code"
              maxLength={6}
              value={code}
              onChange={(e) => setCode(e.target.value.replace(/\D/g, '').slice(0, 6))}
            />
            <Button type="submit" variant="primary" size="sm" disabled={code.length !== 6 || r.busy}>
              {t('settings.account.signIn')}
            </Button>
            <Button variant="quiet" size="sm" onClick={() => setSentTo(null)}>
              {t('settings.account.differentEmail')}
            </Button>
          </div>
        </form>
      )}
      <div className="st-or"><span>{t('settings.account.or')}</span></div>
      <div className="st-acct__actions">
        <Button
          size="sm"
          disabled={r.busy}
          onClick={() => void r.run(signInGoogle).then((ok) => setPending(ok))}
        >
          {t('settings.account.google')}
        </Button>
      </div>
      {pending ? <p className="st-desc" role="status">{t('settings.account.googlePending')}</p> : null}
    </Group>
  );
}

function NeedsSetup({ status, r }: { status: AccountStatus; r: Run }): ReactElement {
  const t = useT();
  const toast = useToast();
  const checkId = useId();
  const [key, setKey] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [copied, copy] = useCopied(copyConcealedText);
  const { run, fail } = r;

  // A new key each time the view opens; nothing is stored until Turn on sync. Not through `run`:
  // StrictMode's second mount would end the first call's busy state while its own is in flight.
  useEffect(() => {
    let live = true;
    createRecoveryKey().then(
      (text) => live && setKey(text),
      (e) => live && fail(e),
    );
    return () => {
      live = false;
    };
  }, []);

  const saveFile = (text: string) =>
    run(async () => {
      const file = await saveTextFile(KEY_FILE_NAME, text);
      if (file) toast.show({ message: t('settings.account.savedFile', { file }) });
    });

  return (
    <Group title={t('settings.account.setupTitle')}>
      {key ? (
        <div className="st-key">
          <span className="st-key__text">{key}</span>
          <Button size="sm" icon={copied ? 'check' : 'copy'} onClick={() => copy(key)}>
            {t(copied ? 'common.copied' : 'common.copy')}
          </Button>
          <Button size="sm" onClick={() => void saveFile(key)}>{t('settings.account.saveFile')}</Button>
        </div>
      ) : null}
      <p className="st-body">{t('settings.account.setupWarning')}</p>
      <label className="st-check" htmlFor={checkId}>
        <input id={checkId} type="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} />
        {t('settings.account.setupConfirm')}
      </label>
      <div className="st-acct__actions">
        <Button variant="primary" size="sm" disabled={!key || !saved || r.busy} onClick={() => void run(confirmRecoveryKey)}>
          {t('settings.account.turnOn')}
        </Button>
      </div>
      <SignedInRow status={status} r={r} />
    </Group>
  );
}

function NeedsRecovery({ status, r }: { status: AccountStatus; r: Run }): ReactElement {
  const t = useT();
  const inputId = useId();
  const [key, setKey] = useState('');
  return (
    <Group title={t('settings.account')}>
      <form
        className="st-acct__form"
        onSubmit={(e) => {
          e.preventDefault();
          if (key.trim() && !r.busy) void r.run(() => enterRecoveryKey(key));
        }}
      >
        <label className="st-row__label" htmlFor={inputId}>{t('settings.account.recoveryLabel')}</label>
        <div className="st-acct__actions">
          <input
            id={inputId}
            className="st-input st-input--key"
            autoComplete="off"
            spellCheck={false}
            value={key}
            onChange={(e) => setKey(e.target.value)}
          />
          <Button type="submit" variant="primary" size="sm" disabled={!key.trim() || r.busy}>
            {t('settings.account.unlock')}
          </Button>
        </div>
        <p className="st-desc">{t('settings.account.recoveryCaption')}</p>
      </form>
      <SignedInRow status={status} r={r} />
    </Group>
  );
}

function Ready({ status, r }: { status: AccountStatus; r: Run }): ReactElement {
  const t = useT();
  const qc = useQueryClient();
  const set = useSetSetting();
  const nameId = useId();
  const maxFileMb = useSetting<number>('sync.maxFileMb', 50);
  const [name, setName] = useState(status.deviceName);
  const [deleting, setDeleting] = useState(false);

  useEffect(() => setName(status.deviceName), [status.deviceName]);

  const saveName = async () => {
    const next = name.trim();
    if (next !== status.deviceName) {
      try {
        // Empty goes back to the Mac's host name.
        await set('sync.deviceName', next || null);
        await qc.invalidateQueries({ queryKey: ACCOUNT_KEY });
      } catch (e) {
        r.fail(e);
      }
    }
    // The field shows the saved name even when the status didn't change (cleared while unset).
    setName(qc.getQueryData<AccountStatus>(ACCOUNT_KEY)?.deviceName ?? status.deviceName);
  };

  return (
    <Group title={t('settings.account')}>
      <SignedInRow status={status} r={r} />
      <SyncRow r={r} />
      <SettingRow label={t('settings.account.deviceName')} htmlFor={nameId}>
        <input
          id={nameId}
          className="st-input st-input--wide"
          autoComplete="off"
          spellCheck={false}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onBlur={() => void saveName()}
          onKeyDown={(e) => {
            if (e.key === 'Enter') e.currentTarget.blur();
          }}
        />
      </SettingRow>
      <p className="st-desc">{t('settings.account.maxFile', { n: maxFileMb })}</p>
      <SettingRow label={t('settings.account.deleteCloud')} desc={t('settings.account.deleteCloudBody')}>
        <Button size="sm" icon="trash" disabled={r.busy} onClick={() => setDeleting(true)}>
          {t('settings.account.deleteCloud')}
        </Button>
      </SettingRow>
      {deleting ? (
        <TypedConfirmDialog
          title={t('settings.account.deleteCloudTitle')}
          body={t('settings.account.deleteCloudBody')}
          phrase={t('settings.account.deleteCloudPhrase')}
          action={t('settings.account.deleteCloud')}
          variant="secondary"
          onConfirm={deleteCloudData}
          failed={(e) => accountError(t, e)}
          onClose={() => setDeleting(false)}
        />
      ) : null}
    </Group>
  );
}

/** "{pill} · Last synced {time}", "{n} changes waiting" and Sync now. */
function SyncRow({ r }: { r: Run }): ReactElement | null {
  const t = useT();
  const lang = useLang();
  const sync = useSyncStatus().data;
  const now = useNow();
  if (!sync) return null;
  const pill = syncPill(sync.state);
  return (
    <div className="st-row">
      <div className="st-row__text">
        <p className="st-body">
          <StatusPill status={pill.status} icon={pill.icon} label={t(pill.label)} />
          {sync.lastSyncedAt != null
            ? ` · ${t('settings.account.lastSynced', { time: relativeTime(sync.lastSyncedAt, now, t, lang) })}`
            : null}
        </p>
        {sync.pending > 0 ? <p className="st-desc">{pendingText(t, sync.pending)}</p> : null}
      </div>
      <Button size="sm" icon="sync" disabled={r.busy || sync.state === 'syncing'} onClick={() => void r.run(syncNow)}>
        {t('sync.now')}
      </Button>
    </div>
  );
}

/** "Signed in as {email}" and Sign out, on every signed-in view. */
function SignedInRow({ status, r }: { status: AccountStatus; r: Run }): ReactElement {
  const t = useT();
  return (
    <SettingRow label={t('settings.account.signedInAs', { email: status.email ?? '' })}>
      <Button size="sm" disabled={r.busy} onClick={() => void r.run(signOut)}>
        {t('settings.account.signOut')}
      </Button>
    </SettingRow>
  );
}
