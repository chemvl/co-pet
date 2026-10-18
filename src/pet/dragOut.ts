// Native drag-out from a tray card (plan Step 7.2). Pointerdown arms it; after 4 px of movement
// the pet plays `handOver` and a real OS drag starts with the formats Rust picks (`prepare_drag`).
// macOS: tauri-plugin-drag, Alt (Option) = as files. Windows: `start_drag_out`, since the plugin
// drags files only there; Ctrl = as files, because Windows targets read Alt during a drag as
// "create a shortcut" and refuse a copy-only drag while it is held.
import { startDrag } from '@crabnebula/tauri-plugin-drag';
import type { PointerEvent as ReactPointerEvent } from 'react';
import { prepareDrag, startDragOut } from '../shared/ipc';
import { isWindows } from '../shared/platform';
import type { ItemSummary } from '../shared/types';
import { renderDragImage } from './dragImage';

export const DRAG_THRESHOLD = 4;

export interface DragOutOptions {
  /** the items to drag: the selection when the card is part of it, else the card */
  items: ItemSummary[];
  onStart(): void;
  /** `dropped` is false when the drag was cancelled or failed */
  onEnd(dropped: boolean): void;
  onError(err: unknown): void;
}

/**
 * Call from a card's `onPointerDown`. Returns nothing; listeners remove themselves
 * on pointerup or once the drag starts.
 */
export function armDragOut(e: ReactPointerEvent, opts: DragOutOptions): void {
  if (e.button !== 0 || opts.items.length === 0) return;
  const x0 = e.clientX;
  const y0 = e.clientY;

  const cleanup = () => {
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', cleanup);
    window.removeEventListener('pointercancel', cleanup);
  };
  const onMove = (ev: PointerEvent) => {
    if (Math.hypot(ev.clientX - x0, ev.clientY - y0) <= DRAG_THRESHOLD) return;
    cleanup();
    void run(isWindows ? ev.ctrlKey : ev.altKey);
  };
  const run = async (asFiles: boolean) => {
    opts.onStart();
    try {
      const ids = opts.items.map((i) => i.id);
      const first = opts.items[0];
      const icon = renderDragImage({ kind: first.kind, meetingId: first.meetingId, title: first.title, count: opts.items.length });
      if (isWindows) {
        opts.onEnd(await startDragOut(ids, asFiles, icon));
        return;
      }
      const plan = await prepareDrag(ids, asFiles);
      await startDrag({ item: plan, icon, mode: 'copy' }, ({ result }) => opts.onEnd(result === 'Dropped'));
    } catch (err) {
      opts.onEnd(false);
      opts.onError(err);
    }
  };

  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', cleanup);
  window.addEventListener('pointercancel', cleanup);
}
