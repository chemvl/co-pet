// A What's Next card (NextCard) rendered as the design-system WhatsNextCard. Shared by the pet
// panel and the Tasks page right column.
import { WhatsNextCard } from '../../ds';
import { categoryColor, categoryIcon, categoryLabel, useCategory } from '../../shared/categories';
import { formatDate, formatEstimate } from '../../shared/format';
import { useLang, useT, type Lang, type TFunction } from '../../shared/i18n';
import type { NextAction, NextCard } from '../../shared/types';
import { dueAt, dueText, isDueSoon } from './groups';

export type CardAction = Exclude<NextAction, 'shown'>;

/** The AI reason when there is one, else the ranker's reason key in words. */
export function reasonText(card: NextCard, now: Date, lang: Lang, t: TFunction): string | undefined {
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

export function NextCardView({ card, onAction }: { card: NextCard; onAction(action: CardAction): void }) {
  const t = useT();
  const lang = useLang();
  const cat = useCategory(card.categoryId);
  const now = new Date();
  return (
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
  );
}
