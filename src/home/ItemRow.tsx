// List rows: the Inbox row (Home.dc.html) and the Unsorted queue row (Unsorted.dc.html).
import { convertFileSrc } from '@tauri-apps/api/core';
import type { ReactElement, Ref } from 'react';
import { CategoryChip, Icon, KIND_ICON, Kbd, StatusPill } from '../ds';
import { useLang, useT } from '../shared/i18n';
import type { CategoryId, ItemSummary } from '../shared/types';
import { categoryKey, rowMeta } from './itemText';
import { categoryMenuItems, MenuButton } from './Menu';

export function KindTile({ item, large }: { item: ItemSummary; large?: boolean }): ReactElement {
  const thumb = !item.sensitive && item.thumbPath ? convertFileSrc(item.thumbPath) : null;
  return (
    <div className={`hm-kt${large ? ' hm-kt--lg' : ''}`}>
      {thumb ? <img src={thumb} alt="" draggable={false} /> : <Icon name={KIND_ICON[item.kind]} size={large ? 20 : 18} />}
    </div>
  );
}

interface RowProps {
  item: ItemSummary;
  active: boolean;
  checked: boolean;
  rowRef: Ref<HTMLDivElement>;
  onToggle: () => void;
  onClick: () => void;
  onOpen: () => void;
}

function RowCheckbox({ item, checked, onToggle }: Pick<RowProps, 'item' | 'checked' | 'onToggle'>): ReactElement {
  const t = useT();
  return (
    <input
      type="checkbox"
      className="hm-cb"
      checked={checked}
      onChange={onToggle}
      onClick={(e) => e.stopPropagation()}
      aria-label={t('home.selectItem', { title: item.title })}
    />
  );
}

function rowClass(base: string, p: Pick<RowProps, 'active' | 'checked'>): string {
  return [base, p.checked && 'is-sel', p.active && 'is-active'].filter(Boolean).join(' ');
}

export function ItemRow(p: RowProps): ReactElement {
  const t = useT();
  const lang = useLang();
  const { item } = p;
  return (
    <div
      ref={p.rowRef}
      id={`row-${item.id}`}
      role="option"
      aria-selected={p.active}
      className={rowClass('hm-row', p)}
      onClick={p.onClick}
      onDoubleClick={p.onOpen}
    >
      <RowCheckbox item={item} checked={p.checked} onToggle={p.onToggle} />
      <KindTile item={item} />
      <div className="hm-row__tx">
        <div className="hm-row__t1">{item.title}</div>
        {item.sensitive ? (
          <div className="hm-row__t2">{t('home.sensitiveSummary')}</div>
        ) : item.summary ? (
          <div className="hm-row__t2">{item.summary}</div>
        ) : null}
        <div className="hm-row__mt">
          {item.triageStatus === 'pending' ? (
            <span className="caption hm-muted">{t('common.sorting')}</span>
          ) : (
            <CategoryChip category={item.categoryId} size="sm" label={t(categoryKey(item.categoryId))} />
          )}
          {item.sensitive ? <StatusPill status="sensitive" label={t('common.sensitive')} /> : null}
          {item.kind === 'screenshot' ? <StatusPill status="local" label={t('common.localOnly')} /> : null}
          <span className="caption hm-muted hm-ellipsis">{rowMeta(item, t, lang)}</span>
        </div>
      </div>
      {item.pinned ? (
        <span className="hm-row__pin">
          <Icon name="pin" size={16} label={t('common.pinned')} />
        </span>
      ) : null}
    </div>
  );
}

export function UnsortedRow(p: RowProps & { onPick: (id: CategoryId) => void }): ReactElement {
  const t = useT();
  const lang = useLang();
  const { item } = p;
  const suggestions = item.suggestions.filter((c) => c !== 'unsorted').slice(0, 2);
  return (
    <div
      ref={p.rowRef}
      id={`row-${item.id}`}
      role="option"
      aria-selected={p.active}
      className={rowClass('hm-row hm-row--unsorted', p)}
      onClick={p.onClick}
      onDoubleClick={p.onOpen}
    >
      <RowCheckbox item={item} checked={p.checked} onToggle={p.onToggle} />
      <KindTile item={item} />
      <div className="hm-row__tx">
        <div className="hm-row__t1">{item.title}</div>
        <div className="caption hm-muted hm-ellipsis">{rowMeta(item, t, lang)}</div>
      </div>
      {suggestions.length ? <span className="hm-sug__lead">{t('home.suggested')}</span> : null}
      {suggestions.map((cat, i) => {
        const category = t(categoryKey(cat));
        return (
          <button
            key={cat}
            type="button"
            className="hm-sug"
            aria-label={t('home.moveToCategory', { category })}
            aria-keyshortcuts={String(i + 1)}
            onClick={(e) => {
              e.stopPropagation();
              p.onPick(cat);
            }}
            onDoubleClick={(e) => e.stopPropagation()}
          >
            <CategoryChip category={cat} size="sm" label={category} />
            <Kbd keys={[String(i + 1)]} />
          </button>
        );
      })}
      <span onDoubleClick={(e) => e.stopPropagation()}>
        <MenuButton
          variant="quiet"
          size="sm"
          iconEnd="chevron-down"
          menuLabel={t('common.moveTo')}
          align="end"
          items={categoryMenuItems(t, p.onPick, item.categoryId)}
        >
          {t('common.other')}
        </MenuButton>
      </span>
    </div>
  );
}
