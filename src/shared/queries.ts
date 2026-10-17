// Shared react-query hooks for settings, categories, the local-model status, the export
// integrations, the MCP clients and the account. Each hook keeps its cache live from Rust
// events: `settings-changed`, `categories-changed`, `integration-changed`, `exports-changed`,
// `agents-changed` and `account-changed` invalidate their query; `ai-status`, `sync-status` and
// `recording-status` replace theirs in place.
import { useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { useCallback, useEffect } from 'react';
import { useT } from './i18n';
import {
  accountStatus, aiStatus, speechStatus, syncStatus, backendStatus, getSettings, integrationsStatus, listCategories, listMcpClients, on,
  recordingStatus,
  setSetting,
} from './ipc';
import type { AiStatus, Json, RecordingStatus, SpeechStatus, SyncStatus } from './types';

export const SETTINGS_KEY = ['settings'] as const;
export const AI_STATUS_KEY = ['aiStatus'] as const;
export const SPEECH_STATUS_KEY = ['speechStatus'] as const;
export const RECORDING_KEY = ['recording'] as const;
export const CATEGORIES_KEY = ['categories'] as const;
export const INTEGRATIONS_KEY = ['integrations'] as const;
export const BACKEND_KEY = ['backendStatus'] as const;
/** Prefix of every Agents screen query (clients, server status, activity). */
export const AGENTS_KEY = ['agents'] as const;
export const MCP_CLIENTS_KEY = [...AGENTS_KEY, 'clients'] as const;
export const ACCOUNT_KEY = ['account'] as const;
export const SYNC_KEY = ['sync'] as const;

export type Settings = Record<string, Json>;

// One event subscription per QueryClient, however many components use the hooks.
const settingsSubscribed = new WeakSet<QueryClient>();
const aiSubscribed = new WeakSet<QueryClient>();
const categoriesSubscribed = new WeakSet<QueryClient>();
const integrationsSubscribed = new WeakSet<QueryClient>();
const agentsSubscribed = new WeakSet<QueryClient>();
const accountSubscribed = new WeakSet<QueryClient>();
const syncSubscribed = new WeakSet<QueryClient>();

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

const speechSubscribed = new WeakSet<QueryClient>();

/** Speech and speaker model packs, kept fresh by `speech-status`. */
export function useSpeechStatus() {
  const qc = useQueryClient();
  useEffect(() => {
    if (speechSubscribed.has(qc)) return;
    speechSubscribed.add(qc);
    void on('speech-status', (s) => {
      qc.setQueryData<SpeechStatus>(SPEECH_STATUS_KEY, s);
    });
  }, [qc]);
  return useQuery({ queryKey: SPEECH_STATUS_KEY, queryFn: speechStatus, staleTime: Infinity });
}

const recordingSubscribed = new WeakSet<QueryClient>();

/** The meeting recorder's state, kept fresh by `recording-status`. */
export function useRecording() {
  const qc = useQueryClient();
  useEffect(() => {
    if (recordingSubscribed.has(qc)) return;
    recordingSubscribed.add(qc);
    void on('recording-status', (s) => {
      qc.setQueryData<RecordingStatus>(RECORDING_KEY, s);
    });
  }, [qc]);
  return useQuery({ queryKey: RECORDING_KEY, queryFn: recordingStatus, staleTime: Infinity });
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

/** The MCP clients; `agents-changed` refreshes every Agents query (`AGENTS_KEY` prefix). */
export function useMcpClients() {
  const qc = useQueryClient();
  useEffect(() => {
    if (agentsSubscribed.has(qc)) return;
    agentsSubscribed.add(qc);
    void on('agents-changed', () => {
      void qc.invalidateQueries({ queryKey: AGENTS_KEY });
    });
  }, [qc]);
  return useQuery({ queryKey: MCP_CLIENTS_KEY, queryFn: listMcpClients, staleTime: Infinity });
}

/** Sign-in and sync-key state (`account_status`). */
export function useAccountStatus() {
  const qc = useQueryClient();
  useEffect(() => {
    if (accountSubscribed.has(qc)) return;
    accountSubscribed.add(qc);
    void on('account-changed', () => {
      void qc.invalidateQueries({ queryKey: ACCOUNT_KEY });
    });
  }, [qc]);
  return useQuery({ queryKey: ACCOUNT_KEY, queryFn: accountStatus, staleTime: Infinity });
}

/** The sync engine's status (`sync_status`), replaced in place by `sync-status`. */
export function useSyncStatus() {
  const qc = useQueryClient();
  useEffect(() => {
    if (syncSubscribed.has(qc)) return;
    syncSubscribed.add(qc);
    void on('sync-status', (s) => qc.setQueryData<SyncStatus>(SYNC_KEY, s));
  }, [qc]);
  return useQuery({ queryKey: SYNC_KEY, queryFn: syncStatus, staleTime: Infinity });
}

/** "Added by {name}" for an `addedBy` client id; an id unknown on this Mac reads "Added by agent". */
export function useAddedBy(): (clientId: string) => string {
  const t = useT();
  const { data: clients } = useMcpClients();
  return useCallback(
    (clientId) => {
      const name = clients?.find((c) => c.id === clientId)?.name;
      return name ? t('common.addedBy', { name }) : t('common.addedByAgent');
    },
    [clients, t],
  );
}
