// One typed wrapper per Rust command (src-tauri/src/commands.rs) and per event.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  AiStatus, SpeechPackId, SpeechStatus, BackendStatus, ExportProvider, ExportTarget, NotionSchema, ProviderStatus, TargetOption, TaskExportState, CaptureResult, Category, CategoryId, CategoryPatch, Counts, DragPlan, EventPayloads, HitRegion,
  HotkeyAction, ItemDetail, ItemEdit, ItemPage, ItemQuery, Json, NewTask, NextAction, NextCard, NextTrigger, PermissionsStatus, CalendarInfo,
  PetStatus, Placement, Rule, RuleView, Example, Edge, TaskCreated, TaskDetail, TaskPatch, TaskQuery, TaskStatus, TaskSummary,
  ActivityRow, HttpClientCreated, KnownMcpClient, McpClientView, McpStatus, McpToken, AccountStatus, SyncStatus, RecordingStatus,
  MeetingDetail, MeetingSummary, DiaryWeek, AiProvider, CloudProvider, KeyProvider, CallDetectionStatus, StorageLocations,
} from './types';

// Step 3 — storage
export const getSettings = () => invoke<Record<string, Json>>('get_settings');
export const setSetting = (key: string, value: Json) => invoke<void>('set_setting', { key, value });
export const listItems = (q: ItemQuery) => invoke<ItemPage>('list_items', { q });
export const getItem = (id: string) => invoke<ItemDetail>('get_item', { id });
export const counts = () => invoke<Counts>('counts');

// Step 4 — pet window
export const setHitRegions = (regions: HitRegion[]) => invoke<void>('set_hit_regions', { regions });
export const setPetMoving = (moving: boolean) => invoke<void>('set_pet_moving', { moving });
export const commitPetPosition = (edge: Edge, offset: number) =>
  invoke<void>('commit_pet_position', { edge, offset });
export const getPlacement = () => invoke<Placement>('get_placement');
export const focusPetPanel = () => invoke<void>('focus_pet_panel');
export const resignPetPanel = () => invoke<void>('resign_pet_panel');

// Step 6 — capture
export const captureClipboard = () => invoke<CaptureResult>('capture_clipboard');
export const captureScreenshot = () => invoke<CaptureResult>('capture_screenshot');
/** Send selection (hotkey): copies the frontmost app's selection, then captures it. */
export const captureSelection = () => invoke<CaptureResult>('capture_selection');
export const captureNote = (text: string) => invoke<CaptureResult>('capture_note', { text });
export const requestPermission = (
  kind: 'screenRecording' | 'accessibility' | 'microphone' | 'calendar',
) => invoke<void>('request_permission', { kind });
export const permissionsStatus = () => invoke<PermissionsStatus>('permissions_status');
export const listCalendars = () => invoke<CalendarInfo[]>('list_calendars');

// Step 7 — tray, copy, paste, drag-out
export const prepareDrag = (ids: string[], asFiles: boolean) =>
  invoke<DragPlan>('prepare_drag', { ids, asFiles });
/** Windows only: prepares and runs the native drag; resolves `true` when a target took the drop. */
export const startDragOut = (ids: string[], asFiles: boolean, icon: string) =>
  invoke<boolean>('start_drag_out', { ids, asFiles, icon });
export const copyItems = (ids: string[]) => invoke<void>('copy_items', { ids });
export const pasteItems = (ids: string[]) => invoke<'ok' | 'needsAccessibility'>('paste_items', { ids });

// Step 9 — triage + AI
export const aiStatus = () => invoke<AiStatus>('ai_status');
export const aiDownload = (modelId: string) => invoke<void>('ai_download', { modelId });
export const aiPause = () => invoke<void>('ai_pause');
export const speechStatus = () => invoke<SpeechStatus>('speech_status');
export const speechDownload = (pack: SpeechPackId) => invoke<void>('speech_download', { pack });
export const speechPause = (pack: SpeechPackId) => invoke<void>('speech_pause', { pack });
export const recordingStatus = () => invoke<RecordingStatus>('recording_status');
/**
 * Rejects with `alreadyRecording`, `invalid`, `noAudioSource` or `micUnavailable`. `callId`: the
 * `call-detected` call being recorded; the recording takes its title and can stop when it ends.
 */
export const startRecording = (language?: string, callId?: string) =>
  invoke<RecordingStatus>('start_recording', { language, callId });
export const pauseRecording = () => invoke<RecordingStatus>('pause_recording');
export const resumeRecording = () => invoke<RecordingStatus>('resume_recording');
/** Resolves with the meeting id; rejects with `notRecording` or `tooShort`. */
export const stopRecording = () => invoke<string>('stop_recording');
export const setRecordingLanguage = (language: string) =>
  invoke<RecordingStatus>('set_recording_language', { language });
/** Settings › Transcribe: the auto-record mode, Meet/Zoom signal sources and the call in progress. */
export const callDetectionStatus = () => invoke<CallDetectionStatus>('call_detection_status');
/** Keep recording on a `call-ended` notice: the call's recording no longer stops with it. */
export const callKeepRecording = (callId: string) => invoke<void>('call_keep_recording', { callId });
/** Resolves with the new meeting id; rejects with `unsupportedAudio`. */
export const importMeeting = (path: string) => invoke<string>('import_meeting', { path });
/** Resolves with the new meeting id; rejects with `invalid` or `unsupportedAudio`. */
export const transcribeItem = (itemId: string) => invoke<string>('transcribe_item', { itemId });
export const listMeetings = () => invoke<MeetingSummary[]>('list_meetings');
export const getMeeting = (id: string) => invoke<MeetingDetail>('get_meeting', { id });
export const renameMeeting = (id: string, title: string) => invoke<void>('rename_meeting', { id, title });
/** `name` null restores the default label. */
export const renameSpeaker = (meetingId: string, key: string, name: string | null) =>
  invoke<void>('rename_speaker', { meetingId, key, name });
/** Rejects with `invalid` unless the meeting failed. */
export const retryMeeting = (id: string) => invoke<void>('retry_meeting', { id });
/** Rejects with `notOnThisMac`, `audioGone`, `busy`, `invalid` or `audioConvert`. */
export const retranscribeMeeting = (id: string, language: string) =>
  invoke<void>('retranscribe_meeting', { id, language });
export const deleteMeeting = (id: string) => invoke<void>('delete_meeting', { id });
/** Re-summarizes a done meeting in `lang` (notes only). Rejects with `busy` or `invalid`. */
export const resummarizeMeeting = (id: string, lang: 'en' | 'vi' | 'zh') =>
  invoke<void>('resummarize_meeting', { id, lang });
/** Queues a skipped or failed summary again. Rejects with `invalid` otherwise. */
export const summarizeMeeting = (id: string) => invoke<void>('summarize_meeting', { id });
/** Rejects with `notFound` (change or task) or `invalid` (not pending). */
export const applyMeetingChange = (meetingId: string, changeId: string) =>
  invoke<{ eventIds: string[] }>('apply_meeting_change', { meetingId, changeId });
/** Rejects with `notFound` or `invalid` (not applied). */
export const undoMeetingChange = (meetingId: string, changeId: string) =>
  invoke<void>('undo_meeting_change', { meetingId, changeId });
/** Rejects with `notFound` or `invalid` (not pending). */
export const ignoreMeetingChange = (meetingId: string, changeId: string) =>
  invoke<void>('ignore_meeting_change', { meetingId, changeId });
/** Rejects with `notFound` or `invalid` (already a task). */
export const addActionToTasks = (meetingId: string, actionId: string) =>
  invoke<TaskSummary>('add_action_to_tasks', { meetingId, actionId });
export const copyMeetingMarkdown = (id: string) => invoke<void>('copy_meeting_markdown', { id });
export const aiSelectModel = (modelId: string) => invoke<void>('ai_select_model', { modelId });
/** Makes `provider` the active AI provider and saves its model / server address.
 *  Rejects with `"invalid"` or `"localOnly"`. */
export const aiConfigure = (provider: AiProvider, model: string | null = null, baseUrl: string | null = null) =>
  invoke<void>('ai_configure', { provider, model, baseUrl });
/** Saves the key in the keychain. Rejects with `"invalid"`. */
export const aiSetKey = (provider: KeyProvider, key: string) => invoke<void>('ai_set_key', { provider, key });
export const aiRemoveKey = (provider: KeyProvider) => invoke<void>('ai_remove_key', { provider });
/** One tiny call with the provider's saved setup. Rejects with `"localOnly"`, `"notSetUp"`, `"unauthorized"`,
 *  `"rateLimited"`, `"network"`, `"serverUnreachable"`, `"aiModel"`, `"aiUnavailable"` or `"aiRequest"`. */
export const aiTest = (provider: CloudProvider) => invoke<void>('ai_test', { provider });
/** The server's model ids, sorted. Rejects with `"localOnly"` or `"serverUnreachable"`. */
export const aiServerModels = (provider: 'ollama' | 'lmstudio') => invoke<string[]>('ai_server_models', { provider });
export const recategorize = (ids: string[], categoryId: CategoryId) =>
  invoke<{ eventIds: string[] }>('recategorize', { ids, categoryId });
export const makeRuleFromItem = (itemId: string) => invoke<Rule>('make_rule_from_item', { itemId });

// Step 10 — system
/** Rejects with `"inUse"` or `"invalid"`. */
export const setHotkey = (action: HotkeyAction, accelerator: string) =>
  invoke<void>('set_hotkey', { action, accelerator });
/** Hotkeys off while the shortcut recorder listens; `set_hotkey` also restores them. */
export const suspendHotkeys = (suspended: boolean) => invoke<void>('suspend_hotkeys', { suspended });
export const quitApp = () => invoke<void>('quit_app');
/** The pet's hidden/sleep state now; call once on mount, then follow `pet-visibility`/`pet-flags`. */
export const petStatus = () => invoke<PetStatus>('pet_status');

// Step 11 — Home
export const openHome = (route?: string) => invoke<void>('open_home', { route: route ?? null });
export const setPinned = (id: string, pinned: boolean) => invoke<{ eventIds: string[] }>('set_pinned', { id, pinned });
/** Rejects with `"invalid"` or `"notFound"`. */
export const editItem = (id: string, edit: ItemEdit) => invoke<{ eventIds: string[] }>('edit_item', { id, edit });
export const archive = (ids: string[]) => invoke<{ eventIds: string[] }>('archive', { ids });
export const trash = (ids: string[]) => invoke<{ eventIds: string[] }>('trash', { ids });
export const undoEvent = (eventId: string) => invoke<void>('undo_event', { eventId });

// Step 12 — Settings
/** Resolves with the appended bundle id, or null when the picker was cancelled. */
export const pickExcludedApp = () => invoke<string | null>('pick_excluded_app');
/** Bundle ids in `capture.excludedApps` with their app names (null when the app isn't installed). */
export const listExcludedApps = () =>
  invoke<{ bundleId: string; name: string | null }[]>('list_excluded_apps');
/** Resolves with the export folder path. */
export const exportAll = (dir: string) => invoke<string>('export_all', { dir });
export const deleteEverything = (confirmation: string) =>
  invoke<void>('delete_everything', { confirmation });
/** Where the models and the data live, and any data move waiting for the next start. */
export const storageLocations = () => invoke<StorageLocations>('storage_locations');
/** Moves the models into `<dir>\Co-pet models` now (null = the default folder). Rejects with `"invalid"`,
 *  `"folderNotWritable"`, `"notEnoughSpace"`, `"transcribing"` or `"moveFailed"`. */
export const setModelsDir = (dir: string | null) => invoke<StorageLocations>('set_models_dir', { dir });
/** Sets the data to move into `<dir>\Co-pet data` at the next start (null = the default folder).
 *  Rejects with `"invalid"`, `"folderInUse"`, `"folderNotWritable"` or `"notEnoughSpace"`. */
export const setDataDir = (dir: string | null) => invoke<StorageLocations>('set_data_dir', { dir });
/** Drops the pending data move. */
export const cancelDataMove = () => invoke<StorageLocations>('cancel_data_move');
/** Restarts Co-pet (a pending data move runs on the way up). Rejects with `"recording"`. */
export const restartApp = () => invoke<void>('restart_app');

// Step 13 — Onboarding
export const finishOnboarding = () => invoke<void>('finish_onboarding');

// Phase 2 M1 — categories. Writes emit `categories-changed`.
export const listCategories = () => invoke<Category[]>('list_categories');
/** Rejects with `"invalid"` or `"duplicate"`. */
export const createCategory = (name: string, icon: string, color: string) =>
  invoke<Category>('create_category', { name, icon, color });
/** Rejects with `"invalid"`, `"duplicate"`, `"notEditable"` or `"notFound"`. */
export const updateCategory = (id: CategoryId, patch: CategoryPatch) =>
  invoke<Category>('update_category', { id, patch });
/** Moves the category's items to Unsorted. Rejects with `"notDeletable"` or `"notFound"`. */
export const deleteCategory = (id: CategoryId) => invoke<{ eventIds: string[] }>('delete_category', { id });
/** `ids` is every live category once; Unsorted stays last. Rejects with `"invalid"`. */
export const reorderCategories = (ids: CategoryId[]) => invoke<void>('reorder_categories', { ids });

// Phase 2 M6 — rules and examples. Writes emit `categories-changed`.
export const listRules = (categoryId: CategoryId) => invoke<RuleView[]>('list_rules', { categoryId });
/** Normalizes `pattern`; rejects with `"invalid"` or `"notFound"`. */
export const addRule = (categoryId: CategoryId, kind: Rule['kind'], pattern: string) =>
  invoke<Rule>('add_rule', { categoryId, kind, pattern });
export const deleteRule = (id: string) => invoke<void>('delete_rule', { id });
/** Undo for `deleteRule`. */
export const restoreRule = (id: string) => invoke<void>('restore_rule', { id });
export const listExamples = (categoryId: CategoryId) => invoke<Example[]>('list_examples', { categoryId });
export const deleteExample = (id: string) => invoke<void>('delete_example', { id });

// Phase 2 M2 — tasks. Writes emit `tasks-changed` then `counts-changed`.
export const listTasks = (q: TaskQuery) => invoke<TaskSummary[]>('list_tasks', { q });
/** Rejects with `"notFound"`. */
export const getTask = (id: string) => invoke<TaskDetail>('get_task', { id });
/** Rejects with `"invalid"` or `"notFound"` (missing source item). */
export const createTask = (input: NewTask) => invoke<TaskCreated>('create_task', { input });
/** Rejects with `"invalid"` or `"notFound"`. */
export const updateTask = (id: string, patch: TaskPatch) =>
  invoke<{ eventIds: string[] }>('update_task', { id, patch });
export const setTaskDone = (id: string, done: boolean) =>
  invoke<{ eventIds: string[] }>('set_task_done', { id, done });
/** `until` is epoch ms. */
export const snoozeTask = (id: string, until: number) => invoke<{ eventIds: string[] }>('snooze_task', { id, until });
export const trashTask = (id: string) => invoke<{ eventIds: string[] }>('trash_task', { id });
/** Rejects with `"notFound"`, `"alreadyUndone"` or `"notUndoable"`. */
export const undoTaskEvent = (eventId: string) => invoke<void>('undo_task_event', { eventId });

// Phase 2 M3 — What's Next
/** `home` uses the ranking only. Resolves with null when no task is eligible. */
export const whatsNext = (trigger: 'bubble' | 'home') => invoke<NextCard | null>('whats_next', { trigger });
/** `start` opens the task's link, file or Home page; `done`/`snooze` emit `tasks-changed`.
 *  Rejects with `"invalid"` or `"notFound"`. */
export const nextAction = (taskId: string, action: NextAction, trigger: NextTrigger) =>
  invoke<void>('next_action', { taskId, action, trigger });

// Phase 2 M4 — export
export const backendStatus = () => invoke<BackendStatus>('backend_status');
/** Tests the user's Supabase project and saves it; another project signs out first. Rejects with
 *  `"localOnly"`, `"invalidUrl"`, `"invalidKey"`, `"secretKey"`, `"backendUnreachable"` or
 *  `"backendRejected"`. */
export const setBackend = (url: string, key: string) => invoke<BackendStatus>('set_backend', { url, key });
/** Forgets the project: signs out and disconnects Notion/Asana connected through it. */
export const clearBackend = () => invoke<BackendStatus>('clear_backend');
export const integrationsStatus = () => invoke<ProviderStatus[]>('integrations_status');
/** Opens the provider's consent page; the result arrives as `integration-changed`.
 *  Rejects with `"localOnly"` or `"notConfigured"`. */
export const connectOauth = (provider: ExportProvider) => invoke<void>('connect_oauth', { provider });
/** Rejects with `"localOnly"`, `"invalidToken"`, `"network"` or `"rateLimited"`. */
export const connectToken = (provider: ExportProvider, token: string) =>
  invoke<ProviderStatus>('connect_token', { provider, token });
/** Deletes the token and fails this device's queued jobs; the target is kept. */
export const disconnect = (provider: ExportProvider) => invoke<void>('disconnect', { provider });
export const notionDataSources = () => invoke<TargetOption[]>('notion_data_sources');
export const notionSchema = (id: string) => invoke<NotionSchema>('notion_schema', { id });
/** `parent` is the workspace gid (`project`) or project gid (`section`). */
export const asanaOptions = (level: 'workspace' | 'project' | 'section', parent?: string) =>
  invoke<TargetOption[]>('asana_options', { level, parent: parent ?? null });
/** `config` is a `NotionConfig` / `AsanaConfig`. Rejects with `"invalid"` or `"notConnected"`. */
export const setExportTarget = (provider: ExportProvider, config: ExportTarget['config'], targetLabel: string) =>
  invoke<ExportTarget>('set_export_target', { provider, config, targetLabel });
/** Rejects with `"notConnected"`, `"noTarget"`, `"alreadyExported"` or `"notFound"`. */
export const exportTask = (taskId: string, provider: ExportProvider) =>
  invoke<TaskExportState>('export_task', { taskId, provider });
/** Requeues this device's failed and queued jobs (of `provider`, or all). */
export const retryExports = (provider?: ExportProvider) => invoke<void>('retry_exports', { provider: provider ?? null });
/** Saves the open or done list through the save panel; the file name, or null when cancelled. */
export const saveTasksFile = (view: TaskStatus, format: 'markdown' | 'csv') => invoke<string | null>('save_tasks_file', { view, format });

// Phase 2 M5 — agents
export const mcpStatus = () => invoke<McpStatus>('mcp_status');
export const setMcpPaused = (paused: boolean) => invoke<void>('set_mcp_paused', { paused });
export const listMcpClients = () => invoke<McpClientView[]>('list_mcp_clients');
/** Writes the client's config. Rejects with `"invalid"` or `"configUnreadable"`. */
export const addMcpClient = (key: KnownMcpClient) => invoke<McpClientView>('add_mcp_client', { key });
/** The token is returned only here. Rejects with `"notFound"`. */
export const approveMcpClient = (id: string) => invoke<McpToken>('approve_mcp_client', { id });
/** Writes off, token cleared. Rejects with `"notFound"`. */
export const revokeMcpClient = (id: string) => invoke<void>('revoke_mcp_client', { id });
/** Removes the `copet` config entry Co-pet wrote. Rejects with `"notFound"` or `"configUnreadable"`. */
export const removeMcpClient = (id: string) => invoke<void>('remove_mcp_client', { id });
/** Key `http:<slug>`, writes approved. Rejects with `"invalid"`. */
export const createHttpClient = (name: string) => invoke<HttpClientCreated>('create_http_client', { name });
export const listAgentActivity = (changesOnly: boolean, limit: number) =>
  invoke<ActivityRow[]>('list_agent_activity', { changesOnly, limit });
/** Rejects with `"notFound"`, `"alreadyUndone"` or `"notUndoable"`. */
export const undoAgentActivity = (id: string) => invoke<void>('undo_agent_activity', { id });

// Phase 2 M8 — account
export const accountStatus = () => invoke<AccountStatus>('account_status');
/** Rejects with `"localOnly"`, `"notConfigured"`, `"network"`, `"rateLimited"` or `"invalid"`. */
export const sendEmailCode = (email: string) => invoke<void>('send_email_code', { email });
/** Signs in and opens Settings › Account. Rejects with `"badCode"`, `"otherAccount"`, `"localOnly"`,
 *  `"notConfigured"` or `"network"`. */
export const verifyEmailCode = (email: string, code: string) => invoke<void>('verify_email_code', { email, code });
/** Opens Google sign-in in the browser; the result arrives as `account-changed`.
 *  Rejects with `"localOnly"` or `"notConfigured"`. */
export const signInGoogle = () => invoke<void>('sign_in_google');
/** The recovery key text; nothing is stored until `confirmRecoveryKey`. Rejects with `"signedOut"`. */
export const createRecoveryKey = () => invoke<string>('create_recovery_key');
/** Rejects with `"invalid"` (no key created), `"keyExists"`, `"signedOut"` or `"network"`. */
export const confirmRecoveryKey = () => invoke<void>('confirm_recovery_key');
/** Rejects with `"badRecoveryKey"`, `"notFound"`, `"signedOut"` or `"network"`. */
export const enterRecoveryKey = (key: string) => invoke<void>('enter_recovery_key', { key });
/** Keeps this Mac's data and `userId`; only the same account can sign in again. */
export const signOut = () => invoke<void>('sign_out');
/** Rejects with `"confirmation"`, `"signedOut"`, `"localOnly"`, `"notConfigured"` or `"network"`. */
export const deleteCloudData = (confirmation: string) => invoke<void>('delete_cloud_data', { confirmation });
/** Shows the save panel at `defaultName` and writes `text` to the picked file. The file name, or
 *  `null` when cancelled. */
export const saveTextFile = (defaultName: string, text: string) =>
  invoke<string | null>('save_text_file', { defaultName, text });
/** Copies a secret marked concealed and transient, so clipboard capture and managers skip it. */
export const copyConcealedText = (text: string) => invoke<void>('copy_concealed_text', { text });

// Phase 2 M9 — sync
export const syncStatus = () => invoke<SyncStatus>('sync_status');
/** Runs a sync (after one in progress) and resolves with the status it ended in. */
export const syncNow = () => invoke<SyncStatus>('sync_now');

// ---------- events ----------
export function on<K extends keyof EventPayloads>(
  event: K,
  handler: (payload: EventPayloads[K]) => void,
): Promise<UnlistenFn> {
  return listen<EventPayloads[K]>(event, (e) => handler(e.payload));
}

// Phase 4 M3 — pet diary
/** null = last week, or this week when last week is empty. Rejects with "invalid". */
export const diaryWeek = (offset: number | null) => invoke<DiaryWeek>('diary_week', { offset });
