import { describe, expect, it } from 'vitest';
import { translate, type TFunction } from '../../shared/i18n';
import type { AiStatus, CloudProviderStatus } from '../../shared/types';
import { activeCloud, isKeyProvider, providerName, testErrorText } from './cloudModel';

const t: TFunction = (key, params) => translate('en', key, params);

const gemini: CloudProviderStatus = { id: 'gemini', hasKey: true, model: 'gemini-3.5-flash-lite', baseUrl: null, models: [], ready: true };
const status = (provider: AiStatus['cloud']['provider']): AiStatus => ({
  modelId: null,
  state: 'none',
  bytesDone: 0,
  bytesTotal: 0,
  error: null,
  models: [],
  recommendedId: '',
  cloud: { provider, providers: [gemini] },
});

describe('cloudModel', () => {
  it('activeCloud is null for local and finds the active provider', () => {
    expect(activeCloud(status('local'))).toBeNull();
    expect(activeCloud(undefined)).toBeNull();
    expect(activeCloud(status('gemini'))).toBe(gemini);
  });
  it('testErrorText maps unauthorized to the refused-key copy', () => {
    expect(testErrorText(t, 'unauthorized')).toBe('The key was refused. Check it and save it again.');
    expect(testErrorText(t, 'somethingOdd')).toBe('somethingOdd');
  });
  it('providerName falls back to the id', () => {
    expect(providerName('x')).toBe('x');
    expect(providerName('lmstudio')).toBe('LM Studio');
  });
  it('isKeyProvider', () => {
    expect(isKeyProvider('openai')).toBe(true);
    expect(isKeyProvider('ollama')).toBe(false);
  });
});
