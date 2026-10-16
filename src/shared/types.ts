// Mirrors src-tauri/src/model.rs and src-tauri/src/events.rs field for field.
export type CategoryId =
  | 'todo' | 'read' | 'reference' | 'code' | 'design' | 'idea' | 'contact' | 'meeting' | 'unsorted';
export const CATEGORY_IDS: readonly CategoryId[] =
  ['todo', 'read', 'reference', 'code', 'design', 'idea', 'contact', 'meeting', 'unsorted'];

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
  byCategory: Partial<Record<CategoryId, number>>;
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

export interface DropPayload { uriList: string; plain: string; html: string; fileNames: string[] }

export type DragPlan = string[] | { data: Record<string, string>; types: string[] };

export interface PermissionsStatus { screenRecording: boolean; accessibility: boolean }

export interface Rule {
  id: string;
  categoryId: CategoryId;
  kind: 'domain' | 'extension' | 'item_kind' | 'source_kind' | 'regex';
  pattern: string;
  builtin: boolean;
  position: number;
}

export type HotkeyAction = 'focusPet' | 'sendSelection' | 'captureScreen' | 'quickNote';

// ---------- events ----------

export type PetInput =
  | 'poof' | 'wake' | 'catch' | 'handOver' | 'camera' | 'confused' | 'idea' | 'tilt' | 'gesture' | 'fidget';

export interface EventPayloads {
  'items-changed': { ids: string[] };
  'counts-changed': Counts;
  'pet-reaction': { input: PetInput; category?: CategoryId; variant?: number };
  'pet-flags': { thinking?: boolean; sleep?: boolean };
  'pointer-region': { region: 'pet' | 'panel' | 'halo' | null; dragging: boolean };
  'pet-visibility': { hidden: boolean; reason: 'fullscreen' | 'user' | null };
  'pet-placement': Placement;
  'ai-status': AiStatus;
  'settings-changed': { key: string };
  hotkey: { action: HotkeyAction };
  navigate: { route: string };
}
