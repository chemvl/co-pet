import { describe, expect, it } from 'vitest';
import { translate, type TFunction } from '../../../shared/i18n';
import type { AccountStatus } from '../../../shared/types';
import { accountError, accountView } from './account';

const t: TFunction = (key, params) => translate('en', key, params);
const vi: TFunction = (key, params) => translate('vi', key, params);

const status = (patch: Partial<AccountStatus>): AccountStatus => ({
  configured: true,
  signedIn: true,
  email: 'a@b.co',
  userId: 'u1',
  keyState: 'ready',
  deviceName: 'Mac',
  lastSyncedAt: null,
  ...patch,
});

describe('accountView', () => {
  it('checks the build before anything else', () => {
    expect(accountView(status({ configured: false }))).toBe('notConfigured');
    expect(accountView(status({ configured: false, signedIn: false, keyState: 'none' }))).toBe('notConfigured');
  });

  it('shows sign-in without a session', () => {
    expect(accountView(status({ signedIn: false, keyState: 'none' }))).toBe('signedOut');
    expect(accountView(status({ signedIn: true, keyState: 'none' }))).toBe('signedOut');
  });

  it('follows the key state once signed in', () => {
    expect(accountView(status({ keyState: 'needsSetup' }))).toBe('needsSetup');
    expect(accountView(status({ keyState: 'needsRecovery' }))).toBe('needsRecovery');
    expect(accountView(status({ keyState: 'ready' }))).toBe('ready');
    expect(accountView(status({ keyState: 'unknown' }))).toBe('unknown');
  });
});

describe('accountError', () => {
  it('has copy for every code the section can receive', () => {
    expect(accountError(t, 'badCode')).toBe("That code didn't work. Check it or send a new one.");
    expect(accountError(t, 'rateLimited')).toBe('Too many attempts. Wait a minute and try again.');
    expect(accountError(vi, 'rateLimited')).toBe('Thử quá nhiều lần. Đợi một phút rồi thử lại.');
    expect(accountError(t, 'otherAccount')).toBe(
      "This computer's data belongs to another Co-pet account. To switch, use Delete everything in Settings › Data first.",
    );
    expect(accountError(t, 'badRecoveryKey')).toBe("That recovery key doesn't match this account.");
    expect(accountError(t, 'keyExists')).toBe('Sync was just turned on from another computer. Enter that recovery key instead.');
    expect(accountError(t, 'signedOut')).toBe('You were signed out. Sign in again to keep syncing.');
    expect(accountError(t, 'decrypt')).toBe("Co-pet couldn't decrypt that data.");
    for (const code of ['oauthExpired', 'network', 'localOnly', 'invalid', 'notFound']) {
      expect(accountError(t, code)).not.toBe(code);
      expect(accountError(vi, code)).not.toBe(code);
    }
  });

  it('passes other failures through', () => {
    expect(accountError(t, new Error('disk full'))).toBe('disk full');
  });
});
