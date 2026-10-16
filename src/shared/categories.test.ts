import { describe, expect, it } from 'vitest';
import { categoryColor, categoryIcon, categoryLabel, categoryLabelOf } from './categories';
import { translate, type TFunction } from './i18n';
import type { Category } from './types';

const t: TFunction = (key, params) => translate('en', key, params);

const row = (over: Partial<Category>): Category => ({
  id: 'read',
  name: null,
  icon: null,
  color: null,
  position: 0,
  excludeFromAi: false,
  excludeFromMcp: false,
  builtin: true,
  ...over,
});

const custom = row({ id: '0190-custom', name: 'Recipes', icon: 'note', color: 'design', builtin: false });

describe('category display', () => {
  it('a built-in without overrides follows its id', () => {
    const read = row({});
    expect(categoryLabel(read, t)).toBe('Read later');
    expect(categoryColor(read)).toBe('read');
    expect(categoryIcon(read)).toBe('read');
  });

  it('a user category uses its own name, icon and colour', () => {
    expect(categoryLabel(custom, t)).toBe('Recipes');
    expect(categoryColor(custom)).toBe('design');
    expect(categoryIcon(custom)).toBe('note');
  });

  it('a colour that is not a built-in id falls back to unsorted', () => {
    expect(categoryColor(row({ id: '0190-x', color: null, builtin: false }))).toBe('unsorted');
  });

  it('labels by id: live row, then built-in name, then raw id', () => {
    expect(categoryLabelOf([custom], custom.id, t)).toBe('Recipes');
    expect(categoryLabelOf([], 'code', t)).toBe('Code snippet');
    expect(categoryLabelOf(undefined, '0190-gone', t)).toBe('0190-gone');
  });
});
