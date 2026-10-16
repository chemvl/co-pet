// One typed wrapper per Rust command (src-tauri/src/commands.rs) and per event.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  AiStatus, CaptureResult, Category, CategoryId, CategoryPatch, Counts, DragPlan, EventPayloads, HitRegion,
  HotkeyAction, ItemDetail, ItemPage, ItemQuery, Json, NewTask, PermissionsStatus, PetStatus, Placement, Rule,
  Edge, TaskCreated, TaskDetail, TaskPatch, TaskQuery, TaskSummary,
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
export const requestPermission = (kind: 'screenRecording' | 'accessibility') =>
  invoke<void>('request_permission', { kind });
export const permissionsStatus = () => invoke<PermissionsStatus>('permissions_status');

// Step 7 — tray, copy, paste, drag-out
export const prepareDrag = (ids: string[], asFiles: boolean) =>
  invoke<DragPlan>('prepare_drag', { ids, asFiles });
export const copyItems = (ids: string[]) => invoke<void>('copy_items', { ids });
export const pasteItems = (ids: string[]) => invoke<'ok' | 'needsAccessibility'>('paste_items', { ids });

// Step 9 — triage + AI
export const aiStatus = () => invoke<AiStatus>('ai_status');
export const aiDownload = (modelId: string) => invoke<void>('ai_download', { modelId });
export const aiPause = () => invoke<void>('ai_pause');
export const aiSelectModel = (modelId: string) => invoke<void>('ai_select_model', { modelId });
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

// ---------- events ----------
export function on<K extends keyof EventPayloads>(
  event: K,
  handler: (payload: EventPayloads[K]) => void,
): Promise<UnlistenFn> {
  return listen<EventPayloads[K]>(event, (e) => handler(e.payload));
}
