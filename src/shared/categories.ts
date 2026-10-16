// How a live category shows: its label, its chip colour and its icon. Built-ins with no
// overrides follow the UI language and the design-system category of the same id.
import type { IconName } from '../ds';
import type { I18nKey, TFunction } from './i18n';
import { useCategories } from './queries';
import { BUILTIN_CATEGORY_IDS, type BuiltinCategoryId, type Category, type CategoryId } from './types';

const isBuiltin = (id: string): id is BuiltinCategoryId =>
  (BUILTIN_CATEGORY_IDS as readonly string[]).includes(id);

export function categoryLabel(c: Category, t: TFunction): string {
  return c.name ?? t(`category.${c.id}` as I18nKey);
}

/**
 * The label of a live category. When it is gone or not loaded yet: a built-in's own name, else
 * the raw id.
 */
export function categoryLabelOf(categories: readonly Category[] | undefined, id: CategoryId, t: TFunction): string {
  const c = categories?.find((x) => x.id === id);
  if (c) return categoryLabel(c, t);
  return isBuiltin(id) ? t(`category.${id}`) : id;
}

/** The design-system category whose colour the chip uses. */
export function categoryColor(c: Category): BuiltinCategoryId {
  const color = c.color ?? c.id;
  return isBuiltin(color) ? color : 'unsorted';
}

export function categoryIcon(c: Category): IconName {
  return (c.icon ?? c.id) as IconName;
}

/** The live category with `id`; undefined while loading or when it is gone. */
export function useCategory(id: CategoryId): Category | undefined {
  return useCategories().data?.find((c) => c.id === id);
}
