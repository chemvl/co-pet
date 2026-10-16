// One typed wrapper per Rust command (src-tauri/src/commands.rs) and per event.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  AiStatus, CaptureResult, CategoryId, Counts, DragPlan, DropPayload, EventPayloads, HitRegion,
  HotkeyAction, ItemDetail, ItemPage, ItemQuery, Json, PermissionsStatus, Placement, Rule, Edge,
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
export const captureDrop = (p: DropPayload) => invoke<CaptureResult>('capture_drop', { p });
export const captureClipboard = () => invoke<CaptureResult>('capture_clipboard');
export const captureScreenshot = () => invoke<CaptureResult>('capture_screenshot');
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
export const quitApp = () => invoke<void>('quit_app');

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

// ---------- events ----------
export function on<K extends keyof EventPayloads>(
  event: K,
  handler: (payload: EventPayloads[K]) => void,
): Promise<UnlistenFn> {
  return listen<EventPayloads[K]>(event, (e) => handler(e.payload));
}
