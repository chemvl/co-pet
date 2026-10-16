// The tray (plan Step 7.1): search, Recent · Pinned · Unsorted, up to 50 cards.
// Cards copy, paste into the focused field, and drag out as real OS drags.
import { convertFileSrc } from '@tauri-apps/api/core';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent, type MouseEvent } from 'react';
import { Button, ICONS, SearchField, SegmentedControl, Tray, TrayCard, type TrayCardLabels } from '../../ds';
import { extOf, hostOf } from '../../home/itemText';
import { categoryColor, categoryIcon, categoryLabel } from '../../shared/categories';
import { formatTime, joinMeta } from '../../shared/format';
import { useLang, useT } from '../../shared/i18n';
import { copyItems, counts as fetchCounts, listItems } from '../../shared/ipc';
import { useAddedBy, useCategories } from '../../shared/queries';
import type { ItemSummary } from '../../shared/types';
import { armDragOut } from '../dragOut';

export type TrayView = 'recent' | 'pinned' | 'unsorted';

export const TRAY_KEY = ['tray'] as const;
export const COUNTS_KEY = ['counts'] as const;
const TRAY_LIMIT = 50;
const COPIED_MS = 1200;

/** The check shown on a card right after Copy (CSS mask; the card markup is the design system's). */
const CHECK_MASK = `url("data:image/svg+xml,${encodeURIComponent(
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">${ICONS.check}</svg>`,
)}")`;

export function TrayPanel(p: {
  /** work-area height; the tray is at most 70% of it */
  areaHeight: number;
  petName: string;
  onClose(): void;
  onPaste(ids: string[]): void;
  onOpenHome(): void;
  onDragStart(): void;
  onDragEnd(dropped: boolean): void;
}) {
  const t = useT();
  const lang = useLang();
  const [view, setView] = useState<TrayView>('recent');
  const [search, setSearch] = useState('');
  const [selected, setSelected] = useState<string[]>([]);
  const [anchor, setAnchor] = useState<string | null>(null);
  const [copied, setCopied] = useState<string[]>([]);
  const searchRef = useRef<HTMLInputElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  const query = useQuery({
    queryKey: [...TRAY_KEY, view, search],
    queryFn: () => listItems({ view, search: search.trim() || undefined, limit: TRAY_LIMIT }),
    placeholderData: keepPreviousData,
  });
  const { data: counts } = useQuery({ queryKey: COUNTS_KEY, queryFn: fetchCounts });
  const { data: categories } = useCategories();
  const addedBy = useAddedBy();
  const items = useMemo(() => query.data?.items ?? [], [query.data]);

  // Selection only covers what is listed.
  useEffect(() => {
    setSelected((sel) => {
      const next = sel.filter((id) => items.some((i) => i.id === id));
      return next.length === sel.length ? sel : next;
    });
  }, [items]);

  useEffect(() => {
    if (copied.length === 0) return;
    const timer = window.setTimeout(() => setCopied([]), COPIED_MS);
    return () => window.clearTimeout(timer);
  }, [copied]);

  const labels: TrayCardLabels = {
    copy: t('common.copy'),
    paste: t('common.pasteIntoField'),
    sensitive: t('common.sensitive'),
    agent: t('common.addedByAgent'),
  };

  const copy = (ids: string[]) => {
    if (ids.length === 0) return;
    copyItems(ids).then(
      () => setCopied(ids),
      (err) => console.error('copy_items failed', err),
    );
  };

  /** Selected items, or the first card when nothing is selected. */
  const targetIds = selected.length > 0 ? selected : items[0] ? [items[0].id] : [];

  const onCardClick = (e: MouseEvent, item: ItemSummary) => {
    if (e.metaKey) {
      setSelected((sel) => (sel.includes(item.id) ? sel.filter((id) => id !== item.id) : [...sel, item.id]));
      setAnchor(item.id);
    } else if (e.shiftKey && anchor) {
      const a = items.findIndex((i) => i.id === anchor);
      const b = items.findIndex((i) => i.id === item.id);
      const [lo, hi] = a < b ? [a, b] : [b, a];
      setSelected(items.slice(Math.max(0, lo), hi + 1).map((i) => i.id));
    } else {
      setSelected([item.id]);
      setAnchor(item.id);
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement;
    const inSearch = target === searchRef.current;
    const onCard = target.classList.contains('cp-card');
    if (e.key === 'Escape') {
      e.preventDefault();
      p.onClose();
    } else if ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && (inSearch || onCard)) {
      if (items.length === 0) return;
      e.preventDefault();
      const current = items.findIndex((i) => i.id === anchor);
      const next =
        e.key === 'ArrowDown'
          ? Math.min(items.length - 1, current + 1)
          : Math.max(0, current - 1);
      const id = items[next].id;
      setSelected([id]);
      setAnchor(id);
      rootRef.current?.querySelector<HTMLElement>(`[data-item-id="${CSS.escape(id)}"] .cp-card`)?.focus();
    } else if (e.key === 'Enter' && (inSearch || onCard)) {
      if (targetIds.length === 0) return;
      e.preventDefault();
      p.onPaste(targetIds);
    } else if (e.key === 'c' && e.metaKey) {
      const input = searchRef.current;
      // Let a text selection inside the search field copy as text.
      if (inSearch && input && input.selectionStart !== input.selectionEnd) return;
      e.preventDefault();
      copy(targetIds);
    } else if (!inSearch && e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) {
      // Typing goes to search.
      searchRef.current?.focus();
    }
  };

  const empty = search.trim() ? t('tray.noResults') : t('tray.empty', { name: p.petName });

  return (
    <div
      className="pt-tray"
      style={{ '--pt-tray-max': `${Math.round(p.areaHeight * 0.7)}px`, '--pt-check': CHECK_MASK } as CSSProperties}
      ref={rootRef}
      onKeyDown={onKeyDown}
    >
      <Tray
        label={t('tray.label')}
        search={
          <SearchField
            ref={searchRef}
            autoFocus
            spellCheck={false}
            autoCorrect="off"
            autoCapitalize="off"
            value={search}
            placeholder={t('tray.searchPlaceholder')}
            onChange={(e) => setSearch(e.target.value)}
          />
        }
        filters={
          <SegmentedControl
            label={t('tray.views')}
            value={view}
            onChange={(v) => setView(v as TrayView)}
            options={[
              { value: 'recent', label: t('common.recent'), count: counts?.inbox },
              { value: 'pinned', label: t('common.pinned'), count: counts?.pinned },
              { value: 'unsorted', label: t('common.unsorted'), count: counts?.unsorted },
            ]}
          />
        }
        footer={
          <>
            <span>{t('tray.altDragHint')}</span>
            <Button variant="quiet" size="sm" icon="home" onClick={p.onOpenHome}>
              {t('common.openHome')}
            </Button>
          </>
        }
      >
        {query.isSuccess && items.length === 0 ? <p className="pt-tray__empty caption">{empty}</p> : null}
        {items.map((item) => {
          const pending = item.triageStatus === 'pending';
          // No chip while sorting or for a category that is gone.
          const cat = pending ? undefined : categories?.find((c) => c.id === item.categoryId);
          const dragIds = selected.includes(item.id) ? selected : [item.id];
          return (
            <div key={item.id} role="listitem" data-item-id={item.id} className="pt-tray__row">
              <TrayCard
                kind={item.kind}
                title={item.title}
                meta={joinMeta([
                  item.sourceApp,
                  item.kind === 'url' ? hostOf(item.url) : extOf(item.fileName),
                  formatTime(item.capturedAt),
                  pending && t('common.sorting'),
                ])}
                category={cat && categoryColor(cat)}
                categoryLabel={cat && categoryLabel(cat, t)}
                categoryIcon={cat && categoryIcon(cat)}
                thumbnail={item.thumbPath && !item.sensitive ? convertFileSrc(item.thumbPath) : undefined}
                pinned={item.pinned}
                sensitive={item.sensitive}
                selected={selected.includes(item.id)}
                lang={lang}
                agent={item.addedBy != null}
                labels={item.addedBy ? { ...labels, agent: addedBy(item.addedBy) } : labels}
                className={copied.includes(item.id) ? 'pt-card is-copied' : 'pt-card'}
                onClick={(e) => onCardClick(e, item)}
                onPointerDown={(e) =>
                  armDragOut(e, {
                    items: items.filter((i) => dragIds.includes(i.id)),
                    onStart: p.onDragStart,
                    onEnd: p.onDragEnd,
                    onError: (err) => console.error('drag-out failed', err),
                  })
                }
                onCopy={() => copy([item.id])}
                onPaste={() => p.onPaste([item.id])}
              />
            </div>
          );
        })}
      </Tray>
    </div>
  );
}
