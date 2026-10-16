// The What's Next card on the pet (plan Task 3.3; docs/design/canvas/Desk.dc.html). Closes only
// through its four actions: Enter Start, D Done, S Snooze, K Skip; Esc snoozes.
import { useEffect, useRef, type KeyboardEvent } from 'react';
import { NextCardView, type CardAction } from '../../shared/NextCardView';
import type { NextCard } from '../../shared/types';

const KEYS: Record<string, CardAction> = { Enter: 'start', d: 'done', s: 'snooze', k: 'skip', Escape: 'snooze' };

export function NextPanel(p: {
  card: NextCard;
  /** Take focus on open (the user asked for the card). */
  autoFocus: boolean;
  onAction(action: CardAction): void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const { card, autoFocus, onAction } = p;

  useEffect(() => {
    if (autoFocus) ref.current?.focus();
  }, [autoFocus]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    // Enter on a focused button activates that button.
    if (e.key === 'Enter' && e.target !== e.currentTarget) return;
    const action = KEYS[e.key.length === 1 ? e.key.toLowerCase() : e.key];
    if (!action) return;
    e.preventDefault();
    // The window-level Esc handler must not close the surface a second time.
    e.stopPropagation();
    onAction(action);
  };

  return (
    <div ref={ref} className="pt-next" tabIndex={-1} onKeyDown={onKeyDown}>
      <NextCardView card={card} onAction={onAction} />
    </div>
  );
}
