// Shared react-query hooks for settings, categories, the local-model status and the export
// integrations. Each hook keeps its cache live from Rust events: `settings-changed`,
// `categories-changed`, `integration-changed` and `exports-changed` invalidate their query;
// `ai-status` replaces the AI status in place.
import { useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { useCallback, useEffect } from 'react';
import { aiStatus, backendStatus, getSettings, integrationsStatus, listCategories, on, setSetting } from './ipc';
import type { AiStatus, Json } from './types';

export const SETTINGS_KEY = ['settings'] as const;
export const AI_STATUS_KEY = ['aiStatus'] as const;
export const CATEGORIES_KEY = ['categories'] as const;
export const INTEGRATIONS_KEY = ['integrations'] as const;
export const BACKEND_KEY = ['backendStatus'] as const;

export type Settings = Record<string, Json>;

// One event subscription per QueryClient, however many components use the hooks.
const settingsSubscribed = new WeakSet<QueryClient>();
const aiSubscribed = new WeakSet<QueryClient>();
const categoriesSubscribed = new WeakSet<QueryClient>();
const integrationsSubscribed = new WeakSet<QueryClient>();

export function useSettings() {
  const qc = useQueryClient();
  useEffect(() => {
    if (settingsSubscribed.has(qc)) return;
    settingsSubscribed.add(qc);
    void on('settings-changed', () => {
      void qc.invalidateQueries({ queryKey: SETTINGS_KEY });
    });
  }, [qc]);
  return useQuery({ queryKey: SETTINGS_KEY, queryFn: getSettings, staleTime: Infinity });
}

/** Reads one setting with a typed fallback while loading or when absent. */
export function useSetting<T extends Json>(key: string, fallback: T): T {
  const { data } = useSettings();
  const v = data?.[key];
  return v === undefined ? fallback : (v as T);
}

/** Writes a setting and updates the cached settings at once, so controls respond immediately. */
export function useSetSetting(): (key: string, value: Json) => Promise<void> {
  const qc = useQueryClient();
  return useCallback(
    async (key, value) => {
      qc.setQueryData<Settings>(SETTINGS_KEY, (old) => ({ ...(old ?? {}), [key]: value }));
      try {
        await setSetting(key, value);
      } finally {
        void qc.invalidateQueries({ queryKey: SETTINGS_KEY });
      }
    },
    [qc],
  );
}

/** Live categories in display order (Unsorted last). */
export function useCategories() {
  const qc = useQueryClient();
  useEffect(() => {
    if (categoriesSubscribed.has(qc)) return;
    categoriesSubscribed.add(qc);
    void on('categories-changed', () => {
      void qc.invalidateQueries({ queryKey: CATEGORIES_KEY });
    });
  }, [qc]);
  return useQuery({ queryKey: CATEGORIES_KEY, queryFn: listCategories, staleTime: Infinity });
}

export function useAiStatus() {
  const qc = useQueryClient();
  useEffect(() => {
    if (aiSubscribed.has(qc)) return;
    aiSubscribed.add(qc);
    void on('ai-status', (s) => {
      qc.setQueryData<AiStatus>(AI_STATUS_KEY, s);
    });
  }, [qc]);
  return useQuery({ queryKey: AI_STATUS_KEY, queryFn: aiStatus, staleTime: Infinity });
}

/** Notion and Asana: connection, target and this device's queue. */
export function useIntegrations() {
  const qc = useQueryClient();
  useEffect(() => {
    if (integrationsSubscribed.has(qc)) return;
    integrationsSubscribed.add(qc);
    const refresh = () => void qc.invalidateQueries({ queryKey: INTEGRATIONS_KEY });
    void on('integration-changed', refresh);
    // Queue counts and the last job error change with every job.
    void on('exports-changed', refresh);
  }, [qc]);
  return useQuery({ queryKey: INTEGRATIONS_KEY, queryFn: integrationsStatus });
}

/** Whether this build can sign in through the backend (OAuth). */
export function useBackendStatus() {
  return useQuery({ queryKey: BACKEND_KEY, queryFn: backendStatus, staleTime: Infinity });
}
