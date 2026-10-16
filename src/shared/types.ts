// Mirrors src-tauri/src/model.rs and src-tauri/src/events.rs field for field.
import type { CategoryId as DsCategoryId } from '../ds';

/** A live category id: one of the built-ins or a user category's UUID. */
export type CategoryId = string;
export type BuiltinCategoryId = DsCategoryId;
/** Built-in ids in design-system order; `unsorted` is last. */
export const BUILTIN_CATEGORY_IDS: readonly BuiltinCategoryId[] =
  ['todo', 'read', 'reference', 'code', 'design', 'idea', 'contact', 'meeting', 'unsorted'];

/**
 * A live `categories` row. `null` name/icon/color: the built-in name `category.<id>`, the icon
 * named like the id, the colour of the id. `color` is otherwise a built-in id.
 */
export interface Category {
  id: CategoryId;
  name: string | null;
  icon: string | null;
  color: string | null;
  position: number;
  excludeFromAi: boolean;
  excludeFromMcp: boolean;
  builtin: boolean;
}

/** `update_category`: omitted fields keep their value. */
export interface CategoryPatch {
  name?: string;
  icon?: string;
  color?: string;
  excludeFromAi?: boolean;
  excludeFromMcp?: boolean;
}

export type ItemKind = 'file' | 'image' | 'url' | 'text' | 'richtext' | 'code' | 'screenshot';
export type SourceKind = 'drop' | 'clipboard' | 'screenshot' | 'note' | 'selection';
export type ExtractStatus = 'pending' | 'done' | 'failed' | 'skipped';
export type TriageSource = 'rule' | 'ai' | 'user' | 'none';
export type ItemStatus = 'inbox' | 'archived' | 'trashed';
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

export interface ItemSummary {
  id: string;
  kind: ItemKind;
  title: string;
  /** null when sensitive */
  summary: string | null;
  tags: string[];
  url: string | null;
  fileName: string | null;
  mime: string | null;
  sizeBytes: number | null;
  /** Absolute path; null when sensitive. Use convertFileSrc. */
  thumbPath: string | null;
  sourceApp: string | null;
  sourceKind: SourceKind;
  categoryId: CategoryId;
  triageStatus: 'pending' | 'done';
  triageSource: TriageSource | null;
  triageConfidence: number | null;
  /** Language-neutral code: `rule:domain:figma.com`, `ai`, `user`, `none:no-model`, … */
  triageReason: string | null;
  suggestions: CategoryId[];
  sensitive: boolean;
  status: ItemStatus;
  pinned: boolean;
  extractStatus: ExtractStatus;
  capturedAt: number;
  updatedAt: number;
  /** `mcp_clients.id` of the agent that added the item; null = the user. */
  addedBy: string | null;
  /** AI-suggested task title; null for rule-decided or non-actionable items. */
  taskSuggestion: string | null;
}

export type EventAction =
  | 'captured' | 'recategorize' | 'pin' | 'unpin' | 'archive' | 'unarchive' | 'trash' | 'restore';

export interface ItemEvent {
  id: string;
  itemId: string;
  at: number;
  actor: 'user' | 'triage';
  action: EventAction;
  beforeJson: Json | null;
  afterJson: Json | null;
  undoneAt: number | null;
}

/** Payload fields are null for sensitive items. */
export interface ItemDetail extends ItemSummary {
  payloadText: string | null;
  payloadHtml: string | null;
  extractedText: string | null;
  /** Absolute path of the stored blob. */
  filePath: string | null;
  events: ItemEvent[];
  /** Live tasks converted from this item. */
  tasks: TaskSummary[];
}

export interface Cursor { capturedAt: number; id: string }

export interface ItemQuery {
  view: 'all' | 'recent' | 'pinned' | 'unsorted';
  categoryId?: CategoryId;
  search?: string;
  sort?: 'newest' | 'oldest';
  limit: number;
  cursor?: Cursor;
}

export interface ItemPage { items: ItemSummary[]; nextCursor: Cursor | null }

export interface Counts {
  inbox: number;
  unsorted: number;
  pinned: number;
  byCategory: Record<string, number>;
  tasksOpen: number;
  tasksDoneToday: number;
  taskDueSoon: boolean;
}

export interface NewTask {
  title: string;
  notes?: string;
  /** 'YYYY-MM-DD' local calendar date. */
  dueDate?: string;
  /** 'HH:MM' local; requires dueDate. */
  dueTime?: string;
  priority?: number;
  estimateMin?: number;
  firstStep?: string;
  sourceItemId?: string;
}

/** Absent = keep; `null` clears the nullable fields. */
export interface TaskPatch {
  title?: string;
  notes?: string;
  dueDate?: string | null;
  dueTime?: string | null;
  priority?: number;
  estimateMin?: number | null;
  firstStep?: string | null;
}

export type TaskStatus = 'open' | 'done';

/** `sourceTitle` is null once the source item is trashed; `categoryId` falls back to 'todo'. */
export interface TaskSummary {
  id: string;
  title: string;
  status: TaskStatus;
  dueDate: string | null;
  dueTime: string | null;
  priority: number;
  estimateMin: number | null;
  firstStep: string | null;
  sourceItemId: string | null;
  sourceTitle: string | null;
  categoryId: CategoryId;
  addedBy: string | null;
  snoozedUntil: number | null;
  doneAt: number | null;
  createdAt: number;
  updatedAt: number;
  /** The most recently updated live export job, if any. */
  export: TaskExportState | null;
}

export type TaskAction = 'created' | 'edited' | 'done' | 'reopened' | 'snoozed' | 'trashed' | 'restored';

export interface TaskEvent {
  id: string;
  at: number;
  actor: 'user' | 'agent' | 'next';
  clientId: string | null;
  action: TaskAction;
  /** JSON text of the changed columns before / after. */
  beforeJson: string | null;
  afterJson: string | null;
  undoneAt: number | null;
}

export interface TaskDetail extends TaskSummary {
  notes: string;
  events: TaskEvent[];
}

export interface TaskQuery { view: TaskStatus; search?: string }

/** `create_task` result; `eventIds` holds the `created` event (for Undo). */
export interface TaskCreated {
  task: TaskSummary;
  eventIds: string[];
}

export type NextTrigger = 'bubble' | 'return' | 'afterDone' | 'agent' | 'home';
export type NextAction = 'shown' | 'start' | 'done' | 'snooze' | 'skip';
export type NextReasonKey =
  | 'overdue' | 'dueSoon' | 'dueToday' | 'dueTomorrow' | 'dueThisWeek' | 'highPriority' | 'waiting';

/** The What's Next card. `reason` is the model's sentence; `reasonKey` the ranking reason. */
export interface NextCard {
  taskId: string;
  title: string;
  reason: string | null;
  reasonKey: NextReasonKey | null;
  firstStep: string | null;
  estimateMin: number | null;
  dueDate: string | null;
  dueTime: string | null;
  categoryId: CategoryId;
  sourceTitle: string | null;
  /** epoch ms: the `waiting` reason's date */
  createdAt: number;
  pickedBy: 'ai' | 'rank';
}

export interface Rect { x: number; y: number; w: number; h: number }
export interface HitRegion extends Rect { kind: 'pet' | 'panel' }
export type Edge = 'left' | 'right' | 'bottom';

export interface Placement {
  monitorName: string;
  edge: Edge;
  /** 0..1 along the edge */
  offset: number;
  scale: number;
  /** logical px */
  workArea: Rect;
}

export interface ModelInfo { id: string; label: string; bytes: number; downloaded: boolean }

export type AiState = 'none' | 'downloading' | 'paused' | 'verifying' | 'ready' | 'failed';
export interface AiStatus {
  modelId: string | null;
  state: AiState;
  bytesDone: number;
  bytesTotal: number;
  error: string | null;
  models: ModelInfo[];
  recommendedId: string;
}

export interface CaptureResult {
  status: 'saved' | 'duplicate' | 'skipped' | 'failed';
  itemIds: string[];
  /** i18n key such as `notice.clipboardExcluded`, or the raw reason for `failed`. */
  message: string | null;
  messageParams: Record<string, string>;
}

export type DragPlan = string[] | { data: Record<string, string>; types: string[] };

export interface PermissionsStatus { screenRecording: boolean; accessibility: boolean }

/** `pet_status`: the current `pet-visibility` and `pet-flags.sleep` values. */
export interface PetStatus { hidden: boolean; reason: 'fullscreen' | 'user' | null; sleep: boolean }

/** `backend_status`: whether this build has a Supabase URL and key. */
export interface BackendStatus { configured: boolean }

export type ExportProvider = 'notion' | 'asana';
export type ExportState = 'queued' | 'exporting' | 'exported' | 'failed';
export type ExportErrorCode = 'rateLimited' | 'network' | 'unauthorized' | 'targetMissing' | 'invalid' | 'disconnected';

export interface TaskExportState {
  provider: ExportProvider;
  state: ExportState;
  error: ExportErrorCode | null;
  externalUrl: string | null;
  nextAttemptAt: number | null;
}

/** `config` is a `NotionConfig` (notion) or `AsanaConfig` (asana). */
export interface ExportTarget {
  id: string;
  provider: ExportProvider;
  accountLabel: string;
  targetLabel: string;
  config: NotionConfig | AsanaConfig;
}

/** `queued` / `failed` / `nextAttemptAt` / `lastError` cover this device's jobs. */
export interface ProviderStatus {
  provider: ExportProvider;
  connected: boolean;
  method: 'oauth' | 'token' | null;
  accountLabel: string | null;
  needsReconnect: boolean;
  target: ExportTarget | null;
  queued: number;
  failed: number;
  nextAttemptAt: number | null;
  lastError: ExportErrorCode | null;
}

/** A Notion data source or an Asana workspace / project / section. */
export interface TargetOption { id: string; label: string }

export interface NotionStatusMap { prop: string; kind: 'status' | 'select'; todo: string; done: string }

/** Notion property names; null = not mapped. */
export interface NotionMapping {
  title: string;
  status: NotionStatusMap | null;
  due: string | null;
  tags: string | null;
  source: string | null;
}

export interface NotionConfig { dataSourceId: string; mapping: NotionMapping }

export interface AsanaConfig { workspaceGid: string; projectGid: string; sectionGid: string | null }

export interface NotionProp { name: string; kind: string; options: string[] }

export interface NotionSchema { title: string; props: NotionProp[]; suggested: NotionMapping }

export interface Rule {
  id: string;
  categoryId: CategoryId;
  kind: 'domain' | 'extension' | 'item_kind' | 'source_kind' | 'source_app' | 'regex';
  pattern: string;
  builtin: boolean;
  position: number;
}

export type HotkeyAction = 'focusPet' | 'sendSelection' | 'captureScreen' | 'quickNote';

// ---------- M5: agents ----------

export type McpHttpState = 'running' | 'stopped' | 'portInUse';

/** `httpUrl` is `http://127.0.0.1:<port>/mcp`; `stdioCommand` is `<copet-mcp path> --stdio`. */
export interface McpStatus { paused: boolean; httpState: McpHttpState; httpUrl: string; stdioCommand: string }

/** The clients "Add to …" configures. */
export type KnownMcpClient = 'claude-code' | 'cursor' | 'codex' | 'gemini-cli';

/** The four known clients are listed with `id: null` until their first contact;
 *  `configInstalled` is false for other and HTTP clients. */
export interface McpClientView {
  id: string | null;
  key: string;
  name: string;
  writeApproved: boolean;
  hasToken: boolean;
  configInstalled: boolean;
  createdAt: number | null;
  approvedAt: number | null;
  lastCallAt: number | null;
}

/** Shown once; only its hash is stored. */
export interface McpToken { token: string }

export interface HttpClientCreated { client: McpClientView; token: string }

/** `undoable`: a write other than `export_task` that recorded changes, undone or not. */
export interface ActivityRow {
  id: string;
  at: number;
  clientName: string;
  tool: string;
  isWrite: boolean;
  detail: Record<string, unknown>;
  resultUrl: string | null;
  error: string | null;
  undoneAt: number | null;
  undoable: boolean;
}

// ---------- events ----------

export type PetInput =
  | 'poof' | 'wake' | 'catch' | 'handOver' | 'camera' | 'confused' | 'idea' | 'tilt' | 'gesture' | 'fidget'
  | 'celebrate' | 'cheer';

export interface EventPayloads {
  'items-changed': { ids: string[] };
  'counts-changed': Counts;
  'pet-reaction': { input: PetInput; category?: CategoryId; variant?: number };
  'pet-flags': { thinking?: boolean; sleep?: boolean; carrying?: boolean };
  'pointer-region': { region: 'pet' | 'panel' | 'halo' | null; dragging: boolean };
  'pet-visibility': { hidden: boolean; reason: 'fullscreen' | 'user' | null };
  'pet-placement': Placement;
  'ai-status': AiStatus;
  'settings-changed': { key: string };
  'categories-changed': Record<string, never>;
  'tasks-changed': { ids: string[] };
  hotkey: { action: HotkeyAction };
  navigate: { route: string };
  /** Pet window only: the outcome of a native drop on the pet. */
  'capture-result': CaptureResult;
  /** Pet window only: a proactive What's Next card. */
  'whats-next': { card: NextCard; trigger: 'return' | 'afterDone' };
  /** Export jobs changed; task lists show `TaskSummary.export`. */
  'exports-changed': { taskIds: string[] };
  /** `provider` is null only for an OAuth handoff this app never started. */
  'integration-changed': {
    provider: ExportProvider | null;
    ok: boolean;
    error: 'oauthExpired' | 'oauthDenied' | string | null;
  };
  /** Agents screen data changed: re-read clients, activity and `mcp_status`. */
  'agents-changed': Record<string, never>;
}
