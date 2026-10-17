// The one place the app renders a category chip: live label, colour and icon.
import type { ReactElement } from 'react';
import { CategoryChip } from '../ds';
import { categoryColor, categoryIcon, categoryLabel, useCategory } from '../shared/categories';
import { useT } from '../shared/i18n';
import type { CategoryId } from '../shared/types';

/** Renders nothing for an id that is not a live category. */
export function CategoryTag({ id, size, iconOnly, count }: { id: CategoryId; size?: 'md' | 'sm'; iconOnly?: boolean; count?: number }): ReactElement | null {
  const t = useT();
  const c = useCategory(id);
  if (!c) return null;
  return (
    <CategoryChip category={categoryColor(c)} icon={categoryIcon(c)} label={categoryLabel(c, t)} size={size} iconOnly={iconOnly} count={count} />
  );
}
