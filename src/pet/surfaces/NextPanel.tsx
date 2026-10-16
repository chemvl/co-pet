// The What's Next card on the pet (plan Task 3.3; docs/design/canvas/Desk.dc.html). Closes only
// through its four actions: Enter Start, D Done, S Snooze, K Skip; Esc snoozes.
import { useEffect, useRef, type KeyboardEvent } from 'react';
import { WhatsNextCard } from '../../ds';
import { dueAt, dueText, isDueSoon } from '../../home/tasks/groups';
import { categoryColor, categoryIcon, categoryLabel, useCategory } from '../../shared/categories';
import { formatDate, formatEstimate } from '../../shared/format';
import { useLang, useT, type Lang, type TFunction } from '../../shared/i18n';
import type { NextAction, NextCard } from '../../shared/types';

export type CardAction = Exclude<NextAction, 'shown'>;

const KEYS: Record<string, CardAction> = { Enter: 'start', d: 'done', s: 'snooze', k: 'skip', Escape: 'snooze' };

function reasonText(card: NextCard, now: Date, lang: Lang, t: TFunction): string | undefined {
  if (card.reason) return card.reason;
  const due = dueAt(card);
  switch (card.reasonKey) {
    case null:
      return undefined;
    case 'overdue':
      return t('next.reason.overdue', { due: dueText(card, now, lang, t) ?? '' });
    case 'dueSoon':
      return t('next.reason.dueSoon', { time: card.dueTime ?? '' });
    case 'dueThisWeek':
      return t('next.reason.dueThisWeek', { day: due ? formatDate(due, lang) : '' });
    case 'waiting':
      return t('next.reason.waiting', { date: formatDate(card.createdAt, lang) });
    default:
      return t(`next.reason.${card.reasonKey}`);
  }
}

export function NextPanel(p: {
  card: NextCard;
  /** Take focus on open (the user asked for the card). */
  autoFocus: boolean;
  onAction(action: CardAction): void;
}) {
  const t = useT();
  const lang = useLang();
  const ref = useRef<HTMLDivElement>(null);
  const cat = useCategory(p.card.categoryId);
  const { card, autoFocus, onAction } = p;
  const now = new Date();

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
    onAction(action);
  };

  return (
    <div ref={ref} className="pt-next" tabIndex={-1} onKeyDown={onKeyDown}>
      <WhatsNextCard
        title={card.title}
        reason={reasonText(card, now, lang, t)}
        estimate={card.estimateMin != null ? formatEstimate(card.estimateMin, lang) : undefined}
        due={dueText(card, now, lang, t)}
        // Cards are open, unsnoozed tasks.
        dueSoon={isDueSoon({ ...card, status: 'open', snoozedUntil: null }, now)}
        category={cat && categoryColor(cat)}
        categoryLabel={cat && categoryLabel(cat, t)}
        categoryIcon={cat && categoryIcon(cat)}
        firstStep={card.firstStep ?? (card.sourceTitle ? t('next.step.openSource', { title: card.sourceTitle }) : undefined)}
        lang={lang}
        labels={{
          eyebrow: t('next.eyebrow'),
          start: t('next.start'),
          done: t('next.done'),
          snooze: t('next.snooze'),
          skip: t('next.skip'),
          firstStep: t('next.firstStep'),
        }}
        onStart={() => onAction('start')}
        onDone={() => onAction('done')}
        onSnooze={() => onAction('snooze')}
        onSkip={() => onAction('skip')}
      />
    </div>
  );
}
