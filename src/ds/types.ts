import type * as React from 'react';

export type IconName =
  | 'whats-next' | 'screenshot' | 'clipboard' | 'mic' | 'home' | 'note'
  | 'todo' | 'read' | 'reference' | 'code' | 'design' | 'idea' | 'contact' | 'meeting' | 'unsorted'
  | 'inbox' | 'tasks' | 'search' | 'copy' | 'paste' | 'pin' | 'more' | 'close' | 'check' | 'plus' | 'chevron-down'
  | 'play' | 'pause' | 'stop' | 'snooze' | 'skip' | 'link' | 'image' | 'file' | 'text' | 'lock' | 'agent'
  | 'sync' | 'export' | 'alert' | 'settings' | 'calendar' | 'clock' | 'language' | 'grip' | 'trash' | 'undo' | 'moon'
  | 'sun' | 'monitor'
  /** Co-pet addition */
  | 'headphones'
  | 'paw';
export type CategoryId = 'todo' | 'read' | 'reference' | 'code' | 'design' | 'idea' | 'contact' | 'meeting' | 'unsorted';
export type ItemKind = 'file' | 'image' | 'url' | 'text' | 'richtext' | 'code' | 'screenshot' | 'audio' | 'meeting';
export type Lang = 'en' | 'vi';
export type Status = 'exported' | 'synced' | 'pending' | 'exporting' | 'failed' | 'due' | 'overdue' | 'local' | 'sensitive' | 'agent';

export interface IconProps { name: IconName; size?: number; label?: string; strokeWidth?: number; className?: string }

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'quiet' | 'danger';
  size?: 'md' | 'sm';
  icon?: IconName;
  iconEnd?: IconName;
}

export interface KbdProps { keys: string[]; className?: string }

export interface CategoryChipProps {
  category: CategoryId;
  label?: string;
  lang?: Lang;
  size?: 'md' | 'sm';
  count?: number;
  iconOnly?: boolean;
  className?: string;
  /** Co-pet addition: overrides the category's icon (user categories pick their own). */
  icon?: IconName;
}

export interface StatusPillProps { status: Status; label?: string; icon?: IconName; className?: string }

export interface PetBadgeProps { count?: number; dot?: boolean; label?: string; className?: string }

/** App addition: `Bubble` forwards its ref to the `<button>` (PetDock bubble focus). */
export interface BubbleProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  icon: IconName;
  label: string;
  showLabel?: boolean;
  labelSide?: 'top' | 'left' | 'right';
  active?: boolean;
}

export interface PetDockProps {
  edge?: 'left' | 'right' | 'bottom';
  state?: 'peek' | 'out' | 'hover' | 'drag' | 'sleep';
  bubbles?: { icon: IconName; label: string }[];
  badge?: number | PetBadgeProps;
  petName?: string;
  size?: number;
  arc?: number;
  focusIndex?: number;
  /** Co-pet addition: that bubble shows its active (hover) state. */
  activeIndex?: number;
  className?: string;
  /** Co-pet addition: renders instead of the internal PetSlot placeholder. */
  art?: React.ReactNode;
  /** Co-pet addition: spread onto the `.cp-pet` button (handlers, aria-label override). */
  petProps?: React.ButtonHTMLAttributes<HTMLButtonElement>;
  /** Co-pet addition: wired to each bubble's onClick. */
  onBubble?: (icon: IconName, index: number) => void;
  /** Co-pet addition: replaces the hard-coded 'Pet actions' toolbar label. */
  bubblesLabel?: string;
  /** Co-pet addition: exposes the bubble buttons for arrow-key focus. */
  bubbleFocusRef?: (index: number, el: HTMLButtonElement | null) => void;
}

/**
 * App addition: `SearchField` forwards its ref to the `<input>` (Cmd+K focus). The HTML `size`
 * attribute is omitted so `size` can carry the design-system 'md' | 'lg' variant.
 */
export interface SearchFieldProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'size'> { shortcut?: string[]; size?: 'md' | 'lg' }

export interface SegmentedControlProps {
  options: { value: string; label: string; icon?: IconName; count?: number }[];
  value: string;
  onChange?: (value: string) => void;
  label?: string;
  className?: string;
}

export interface SwitchProps { label: string; description?: string; checked?: boolean; onChange?: (checked: boolean) => void; disabled?: boolean; id?: string; className?: string }

export interface TrayCardLabels { copy: string; paste: string; sensitive: string; agent: string }

export interface TrayCardProps
  extends Pick<React.HTMLAttributes<HTMLElement>, 'onPointerDown' | 'onClick' | 'onKeyDown' | 'onDoubleClick' | 'id' | 'onMouseEnter'> {
  kind: ItemKind;
  title: string;
  meta?: string;
  category?: CategoryId;
  thumbnail?: string;
  pinned?: boolean;
  selected?: boolean;
  sensitive?: boolean;
  agent?: boolean;
  dragging?: boolean;
  lang?: Lang;
  onCopy?: () => void;
  onPaste?: () => void;
  /** Co-pet addition: localized aria-labels and pill labels. */
  labels?: TrayCardLabels;
  /** Co-pet addition: the category chip's label and icon (user categories). */
  categoryLabel?: string;
  /** Co-pet addition: see `categoryLabel`. */
  categoryIcon?: IconName;
  className?: string;
}

export interface TrayProps { search?: React.ReactNode; filters?: React.ReactNode; footer?: React.ReactNode; label?: string; children?: React.ReactNode; className?: string }

export interface WhatsNextCardProps {
  title: string;
  reason?: string;
  estimate?: string;
  due?: string;
  dueSoon?: boolean;
  category?: CategoryId;
  /** Co-pet addition: the category chip's label and icon (user categories). */
  categoryLabel?: string;
  /** Co-pet addition: see `categoryLabel`. */
  categoryIcon?: IconName;
  firstStep?: string;
  lang?: Lang;
  labels?: Partial<{ eyebrow: string; start: string; done: string; snooze: string; skip: string; firstStep: string }>;
  onStart?: () => void;
  onDone?: () => void;
  onSnooze?: () => void;
  onSkip?: () => void;
  className?: string;
}

export interface RecordingHUDProps {
  elapsed?: string;
  micLevel?: number;
  systemLevel?: number;
  language?: string;
  paused?: boolean;
  /** Co-pet addition: `language` is the language button's aria-label. */
  labels?: Partial<{ me: string; system: string; pause: string; resume: string; stop: string; recording: string; paused: string; language: string }>;
  onPause?: () => void;
  onStop?: () => void;
  /** Co-pet addition: called by the language button with that button as the menu anchor. */
  onLanguage?: (anchor: HTMLButtonElement) => void;
  className?: string;
}

export interface TaskRowProps {
  title: string;
  done?: boolean;
  onToggle?: () => void;
  source?: 'item' | 'meeting';
  sourceLabel?: string;
  estimate?: string;
  due?: string;
  dueSoon?: boolean;
  overdue?: boolean;
  exportStatus?: Status;
  exportLabel?: string;
  className?: string;
  /** Co-pet addition: click on the body or Enter on the focused row; also makes the row focusable. */
  onOpen?: () => void;
  /** Co-pet addition: called by the quiet "more" button with that button as the menu anchor. */
  onActions?: (anchor: HTMLButtonElement) => void;
  /** Co-pet addition: adds `is-selected` (surface-selected background). */
  selected?: boolean;
  /** Co-pet addition: replaces the English aria-labels of the check and the "more" button. */
  labels?: { markDone: string; actions: string };
  /** Co-pet addition: makes the export pill a button (e.g. open the exported page). */
  onExportClick?: () => void;
}

export interface TranscriptSegmentProps {
  start: string;
  speaker: string;
  isMe?: boolean;
  speakerIndex?: number;
  language?: string;
  active?: boolean;
  taskChange?: string;
  text?: string;
  children?: React.ReactNode;
  className?: string;
  /** Co-pet addition: the time button plays from `start`. */
  onPlay?: () => void;
  /** Co-pet addition: the time button's aria-label (replaces the English 'Play from …'). */
  playLabel?: string;
  /** Co-pet addition: renders the speaker chip as a button, called with it as the menu anchor. */
  onSpeaker?: (anchor: HTMLButtonElement) => void;
  /** Co-pet addition: the speaker button's aria-label. */
  speakerLabel?: string;
}

