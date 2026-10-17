// Pure helpers for the Cloud providers card (CloudProvidersCard.tsx).
import { errorMessage, errorText } from '../../shared/errors';
import type { TFunction } from '../../shared/i18n';
import type { AiStatus, CloudProvider, CloudProviderStatus, KeyProvider } from '../../shared/types';

/** Product names, not translated. */
export const AI_PROVIDER_NAME: Record<CloudProvider, string> = {
  anthropic: 'Anthropic',
  openai: 'OpenAI',
  gemini: 'Gemini',
  ollama: 'Ollama',
  lmstudio: 'LM Studio',
};

export const KEY_PLACEHOLDER: Record<KeyProvider, string> = {
  anthropic: 'sk-ant-…',
  openai: 'sk-…',
  gemini: 'AIza…',
};

export function isKeyProvider(p: string): p is KeyProvider {
  return p === 'anthropic' || p === 'openai' || p === 'gemini';
}

/** The selected cloud provider's status; null while the local model is the provider. */
export function activeCloud(s: AiStatus | undefined): CloudProviderStatus | null {
  if (!s || s.cloud.provider === 'local') return null;
  return s.cloud.providers.find((p) => p.id === s.cloud.provider) ?? null;
}

export function providerName(id: string): string {
  return (AI_PROVIDER_NAME as Record<string, string>)[id] ?? id;
}

export function testErrorText(t: TFunction, e: unknown): string {
  return errorText(e) === 'unauthorized' ? t('integrations.ai.keyRefused') : errorMessage(t, e);
}
