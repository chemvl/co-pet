import { useQueryClient } from '@tanstack/react-query';
import { save } from '@tauri-apps/plugin-dialog';
import { useEffect, useId, useState, type ReactElement } from 'react';
import { Button } from '../../../ds';
import { useT } from '../../../shared/i18n';
import {
  confirmRecoveryKey, createRecoveryKey, deleteCloudData, enterRecoveryKey, on, sendEmailCode, signInGoogle, signOut,
  verifyEmailCode, writeTextFile,
} from '../../../shared/ipc';
import { ACCOUNT_KEY, useAccountStatus, useSetSetting, useSetting } from '../../../shared/queries';
import { useToast } from '../../../shared/Toast';
import type { AccountStatus } from '../../../shared/types';
import { Group, SettingRow, TypedConfirmDialog, useCopied } from '../parts';
import { accountError, accountView } from './account';

const KEY_FILE_NAME = 'Co-pet recovery key.txt';

/** Runs one account action at a time: clears the error first, shows a failure as `error`. */
interface Run {
  busy: boolean;
  run: (op: () => Promise<unknown>) => Promise<boolean>;
}

export function AccountSection({ onNavigate }: { onNavigate: (route: string) => void }): ReactElement | null {
  const t = useT();
  const qc = useQueryClient();
  const { data: status } = useAccountStatus();
  const localOnly = useSetting<boolean>('privacy.localOnly', false);
  const [busy, setBusy] = useState(false);
  // Kept across state changes: `keyExists` is meant to be read on the recovery-key view it leads to.
  const [error, setError] = useState<string | null>(null);

  // Re-read on open and whenever Local-only mode changes what the server can be asked.
  useEffect(() => {
    void qc.invalidateQueries({ queryKey: ACCOUNT_KEY });
  }, [qc, localOnly]);

  const run: Run['run'] = async (op) => {
    setBusy(true);
    setError(null);
    try {
      await op();
      return true;
    } catch (e) {
      setError(accountError(t, e));
      // A refused step may mean the state moved (signed out, another Mac turned sync on).
      void qc.invalidateQueries({ queryKey: ACCOUNT_KEY });
      return false;
    } finally {
      setBusy(false);
    }
  };

  if (!status) return null;
  const view = accountView(status);
  if (view === 'notConfigured') {
    return (
      <Group title={t('settings.account')}>
        <p className="st-body">{t('settings.account.notConfigured')}</p>
      </Group>
    );
  }

  const r: Run = { busy, run };
  return (
    <>
      {localOnly ? (
        <div className="st-row">
          <p className="st-body st-row__text">{t('settings.account.localOnly')}</p>
          <Button size="sm" onClick={() => onNavigate('settings/privacy')}>{t('settings.account.openPrivacy')}</Button>
        </div>
      ) : null}
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
    </>
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
  const [copied, copy] = useCopied();
  const { run } = r;

  // A new key each time the view opens; nothing is stored until Turn on sync.
  useEffect(() => {
    let live = true;
    void run(async () => {
      const text = await createRecoveryKey();
      if (live) setKey(text);
    });
    return () => {
      live = false;
    };
  }, []);

  const saveFile = async (text: string) => {
    const path = await save({ defaultPath: KEY_FILE_NAME });
    if (!path) return;
    if (await run(() => writeTextFile(path, text))) {
      toast.show({ message: t('settings.account.savedFile', { file: path.split('/').pop() ?? path }) });
    }
  };

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
    if (next === status.deviceName) return;
    // Empty goes back to the Mac's host name.
    await set('sync.deviceName', next || null);
    void qc.invalidateQueries({ queryKey: ACCOUNT_KEY });
  };

  return (
    <Group title={t('settings.account')}>
      <SignedInRow status={status} r={r} />
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
