// Shared react-query hooks for settings and the local-model status.
// Each hook keeps its cache live from Rust events: `settings-changed` invalidates
// the settings query; `ai-status` replaces the AI status in place.
import { useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { useCallback, useEffect } from 'react';
import { aiStatus, getSettings, on, setSetting } from './ipc';
import type { AiStatus, Json } from './types';

export const SETTINGS_KEY = ['settings'] as const;
export const AI_STATUS_KEY = ['aiStatus'] as const;

export type Settings = Record<string, Json>;

// One event subscription per QueryClient, however many components use the hooks.
const settingsSubscribed = new WeakSet<QueryClient>();
const aiSubscribed = new WeakSet<QueryClient>();

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
