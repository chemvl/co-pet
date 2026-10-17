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
  /** The meeting this item summarizes. */
  meetingId: string | null;
  /** False when the item's library file isn't on this Mac (synced from another Mac, not
   *  downloaded yet or over the size limit): drag-out, Copy, Paste and Open file can't use it. */
  fileAvailable: boolean;
  /** The item's file is over `sync.maxFileMb` and never leaves this Mac. */
  fileTooLarge: boolean;
}

export type EventAction =
  | 'captured' | 'recategorize' | 'tag' | 'pin' | 'unpin' | 'archive' | 'unarchive' | 'trash' | 'restore';

export interface ItemEvent {
  id: string;
  itemId: string;
  at: number;
  actor: 'user' | 'triage' | 'agent';
  clientId: string | null;
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
  /** The source item's meeting. */
  sourceMeetingId: string | null;
  categoryId: CategoryId;
  addedBy: string | null;
  snoozedUntil: number | null;
  doneAt: number | null;
  createdAt: number;
  updatedAt: number;
  /** The most recently updated live export job, if any. */
  export: TaskExportState | null;
}

/** One transcript segment; `speaker` is a `Speaker.key`. */
export interface TranscriptSeg {
  startMs: number;
  endMs: number;
  speaker: string;
  text: string;
  lang: 'en' | 'vi' | 'zh';
}

/** `key`: `me` | `others` | `s1`, `s2`…; `name` null = the default label. */
export interface Speaker {
  key: string;
  name: string | null;
}

/** `due` is `YYYY-MM-DD`; `taskId` is set once the item became a Task. */
export interface ActionItem {
  id: string;
  text: string;
  owner: string | null;
  ownerKey: string | null;
  due: string | null;
  atMs: number | null;
  taskId: string | null;
}

/** A change a meeting proposes to an existing task. */
export interface TaskChange {
  id: string;
  taskId: string;
  field: 'due' | 'title' | 'done';
  from: string | null;
  to: string | null;
  atMs: number | null;
  state: 'pending' | 'applied' | 'ignored';
  eventIds: string[];
}

export type MeetingLanguage = 'auto' | 'en' | 'vi' | 'zh' | 'vi+en';
export type MeetingState = 'recording' | 'queued' | 'transcribing' | 'done' | 'failed';
export type SummaryState = 'none' | 'pending' | 'running' | 'done' | 'skipped' | 'failed';
export type MeetingAudio = 'available' | 'processing' | 'deleted' | 'elsewhere';

export interface MeetingSummary {
  id: string;
  title: string;
  source: 'recording' | 'import';
  startedAt: number;
  durationMs: number;
  language: MeetingLanguage;
  langs: string[];
  state: MeetingState;
  /** 0–1 while this meeting is being transcribed. */
  progress: number | null;
  error: string | null;
  summaryState: SummaryState;
  speakerCount: number;
  actionCount: number;
  tasksAdded: number;
  itemId: string | null;
  onThisDevice: boolean;
  audio: MeetingAudio;
}

export interface MeetingDetail extends MeetingSummary {
  speakers: Speaker[];
  transcript: TranscriptSeg[];
  summary: string[];
  decisions: string[];
  questions: string[];
  actions: ActionItem[];
  changes: TaskChange[];
  summaryLang: string | null;
  summaryError: string | null;
  /** Absolute `mix.m4a` when `audio` is `available`. */
  audioPath: string | null;
}

/** `recording-status`. */
export interface RecordingStatus {
  state: 'idle' | 'recording' | 'paused';
  meetingId: string | null;
  startedAt: number | null;
  /** Active time (pauses excluded) when this status was sent. */
  elapsedMs: number;
  language: MeetingLanguage;
  /** The track is capturing. */
  mic: boolean;
  system: boolean;
  /** Only on the transition to `idle`. */
  endedReason: 'maxLength' | 'tooShort' | null;
}

export type TaskAction = 'created' | 'edited' | 'done' | 'reopened' | 'snoozed' | 'trashed' | 'restored';

export interface TaskEvent {
  id: string;
  at: number;
  actor: 'user' | 'agent' | 'next' | 'meeting';
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

export type NextTrigger = 'bubble' | 'return' | 'afterDone' | 'afterMeeting' | 'agent' | 'home';
export type NextAction = 'shown' | 'start' | 'done' | 'snooze' | 'skip';
export type NextReasonKey =
  | 'overdue' | 'dueSoon' | 'dueToday' | 'dueTomorrow' | 'dueThisWeek' | 'highPriority' | 'waiting'
  | 'fitsBeforeMeeting';

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
  /** epoch ms: the next calendar event, with `fitsBeforeMeeting` */
  nextEventAt: number | null;
  pickedBy: 'ai' | 'rank';
}

export interface Rect { x: number; y: number; w: number; h: number }
/** `zone` reports the pointer without taking it; a hit panel's `id` comes back as the event's `target`. */
export interface HitRegion extends Rect { kind: 'pet' | 'panel' | 'zone'; id?: string }
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

export type AiProvider = 'local' | 'anthropic' | 'openai' | 'gemini' | 'ollama' | 'lmstudio';
export type CloudProvider = Exclude<AiProvider, 'local'>;
export type KeyProvider = 'anthropic' | 'openai' | 'gemini';
export interface CloudModelInfo { id: string; label: string }
export interface CloudProviderStatus {
  id: CloudProvider;
  hasKey: boolean;
  model: string | null;
  baseUrl: string | null;
  /** Curated models; empty for Ollama / LM Studio. */
  models: CloudModelInfo[];
  /** A model is set and, for key providers, a key is saved. */
  ready: boolean;
}
/** `providers` in Anthropic, OpenAI, Gemini, Ollama, LM Studio order. */
export interface CloudStatus { provider: AiProvider; providers: CloudProviderStatus[] }

export interface AiStatus {
  modelId: string | null;
  state: AiState;
  bytesDone: number;
  bytesTotal: number;
  error: string | null;
  models: ModelInfo[];
  recommendedId: string;
  cloud: CloudStatus;
}

export type SpeechPackId = 'speech' | 'speaker';
export interface PackStatus {
  id: SpeechPackId;
  state: AiState;
  bytesDone: number;
  bytesTotal: number;
  error: string | null;
}
/** Speech pack first, then speaker. */
export interface SpeechStatus { packs: PackStatus[] }

export interface CaptureResult {
  status: 'saved' | 'duplicate' | 'skipped' | 'failed';
  itemIds: string[];
  /** i18n key such as `notice.clipboardExcluded`, or the raw reason for `failed`. */
  message: string | null;
  messageParams: Record<string, string>;
  /** A dropped audio or video file the pet offers to transcribe (`notice.offerTranscribe`). */
  transcribeItemId: string | null;
}

export type DragPlan = string[] | { data: Record<string, string>; types: string[] };

export interface PermissionsStatus {
  screenRecording: boolean;
  accessibility: boolean;
  microphone: 'granted' | 'denied' | 'notDetermined';
  calendar: 'granted' | 'denied' | 'notDetermined';
}

/** `list_calendars`: one event calendar on this Mac; `id` is EventKit's `calendarIdentifier`. */
export interface CalendarInfo { id: string; title: string; account: string }

/** `pet_status`: the current `pet-visibility` and `pet-flags.sleep` values. */
export interface PetStatus { hidden: boolean; reason: 'fullscreen' | 'user' | null; sleep: boolean }

/** `backend_status`: whether this build has a Supabase URL and key. */
export interface BackendStatus { configured: boolean }

/** Where this Mac stands with the sync master key. */
export type KeyState = 'none' | 'ready' | 'needsSetup' | 'needsRecovery' | 'unknown';

/** `account_status`. `userId` stays set after sign-out; `deviceName` is `sync.deviceName`, else
 *  the Mac's host name; `lastSyncedAt` comes from the sync engine (M9). */
export interface AccountStatus {
  configured: boolean;
  signedIn: boolean;
  email: string | null;
  userId: string | null;
  keyState: KeyState;
  deviceName: string;
  lastSyncedAt: number | null;
}

export type SyncState = 'off' | 'paused' | 'needsKey' | 'signedOut' | 'syncing' | 'idle' | 'offline' | 'error';

/** `sync_status` / `sync-status`. `pending`: changes waiting to upload; `blobsPending`: files
 *  waiting to upload or download. */
export interface SyncStatus {
  state: SyncState;
  lastSyncedAt: number | null;
  pending: number;
  blobsPending: number;
}

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

/** A rule in the editor; `matchCount` = live items of its category it matches. */
export interface RuleView extends Rule {
  matchCount: number;
}

/** A correction example that teaches the local model. */
export interface Example {
  id: string;
  itemId: string;
  title: string;
  createdAt: number;
}

export type HotkeyAction = 'focusPet' | 'sendSelection' | 'captureScreen' | 'quickNote' | 'recordMeeting';

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
  | 'celebrate' | 'cheer' | 'glance';

export interface EventPayloads {
  'items-changed': { ids: string[] };
  'counts-changed': Counts;
  'pet-reaction': { input: PetInput; category?: CategoryId; variant?: number };
  'pet-flags': { thinking?: boolean; sleep?: boolean; carrying?: boolean };
  'pointer-region': { region: 'pet' | 'panel' | 'halo' | 'zone' | null; target: string | null; dragging: boolean };
  'pet-visibility': { hidden: boolean; reason: 'fullscreen' | 'user' | null };
  'pet-placement': Placement;
  'ai-status': AiStatus;
  'speech-status': SpeechStatus;
  'settings-changed': { key: string };
  'categories-changed': Record<string, never>;
  'tasks-changed': { ids: string[] };
  'meetings-changed': { ids: string[] };
  hotkey: { action: HotkeyAction };
  navigate: { route: string };
  /** Pet window only: the outcome of a native drop on the pet. */
  'capture-result': CaptureResult;
  /** Pet window only: a proactive What's Next card. */
  'whats-next': { card: NextCard; trigger: 'return' | 'afterDone' | 'afterMeeting' };
  /** Pet window only: last week's diary is ready; `week` = this week's Monday. */
  'diary-ready': { week: string };
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
  /** Sign-in, sign-out, the sync key or cloud data changed: re-read `account_status`.
   *  `error` is set only when Google sign-in failed. */
  'account-changed': { error: 'oauthExpired' | 'otherAccount' | string | null };
  /** The sync engine's state changed or a sync finished. */
  'sync-status': SyncStatus;
  /** The recorder's state changed. */
  'recording-status': RecordingStatus;
  /** Pet window only, every 100 ms while recording: track levels 0–1 (0 for a track that is off). */
  'recording-level': { mic: number; system: number };
}

// Phase 4 M3 — pet diary
export interface DiaryDay { date: string; captured: number; tasksDone: number; meetings: number }
export interface DiaryCategory { categoryId: CategoryId; count: number }
export interface DiaryWeek {
  offset: number;
  start: string;
  end: string;
  captured: number;
  tasksDone: number;
  meetings: number;
  meetingMs: number;
  topCategories: DiaryCategory[];
  days: DiaryDay[];
  hasPrevious: boolean;
}
