import { describe, expect, it } from 'vitest';
import { translate, type Lang, type TFunction } from '../../shared/i18n';
import type { RuleView } from '../../shared/types';
import { ruleRows, ruleText } from './RuleSentence';

const tr = (lang: Lang): TFunction => (key, params) => translate(lang, key, params);
const en = tr('en');
const vi = tr('vi');

let seq = 0;
function rule(kind: RuleView['kind'], pattern: string, matchCount = 1): RuleView {
  seq += 1;
  return { id: `r${seq}`, categoryId: 'design', kind, pattern, builtin: false, position: seq, matchCount };
}

const texts = (rules: RuleView[], t: TFunction) => ruleRows(rules, t).map(ruleText);

describe('ruleRows', () => {
  it('writes each kind as a sentence in English', () => {
    expect(
      texts(
        [
          rule('domain', 'figma.com'),
          rule('extension', 'png'),
          rule('item_kind', 'url'),
          rule('source_kind', 'note'),
          rule('source_app', 'Figma'),
          rule('regex', '^todo'),
        ],
        en,
      ),
    ).toEqual([
      'URL domain is figma.com',
      'File type is png',
      'Item is Link',
      'Captured by Quick note',
      'Source app is Figma',
      'Text matches ^todo',
    ]);
  });

  it('writes each kind as a sentence in Vietnamese', () => {
    expect(
      texts(
        [
          rule('domain', 'figma.com'),
          rule('extension', 'png'),
          rule('item_kind', 'url'),
          rule('source_kind', 'note'),
          rule('source_app', 'Figma'),
          rule('regex', '^todo'),
        ],
        vi,
      ),
    ).toEqual([
      'Tên miền URL là figma.com',
      'Loại file là png',
      'Mục là Liên kết',
      'Được lưu qua Ghi chú nhanh',
      'Ứng dụng nguồn là Figma',
      'Nội dung khớp ^todo',
    ]);
  });

  it('splits the sentence around the value, user text in mono', () => {
    const [domain, kind] = ruleRows([rule('domain', 'figma.com'), rule('item_kind', 'image')], en);
    expect(domain).toMatchObject({ before: 'URL domain is ', value: 'figma.com', after: '', mono: true });
    expect(kind).toMatchObject({ before: 'Item is ', value: 'Image', mono: false });
  });

  it('groups consecutive file types into one row with the summed count and every id', () => {
    const png = rule('extension', 'png', 2);
    const jpg = rule('extension', 'jpg', 3);
    const svg = rule('extension', 'svg', 0);
    const rows = ruleRows([png, jpg, svg], en);
    expect(rows).toHaveLength(1);
    expect(ruleText(rows[0])).toBe('File type is any of png, jpg, svg');
    expect(rows[0].ids).toEqual([png.id, jpg.id, svg.id]);
    expect(rows[0].matchCount).toBe(5);
    expect(texts([png, jpg], vi)).toEqual(['Loại file là một trong png, jpg']);
  });

  it('does not group file types that are not consecutive', () => {
    const rows = ruleRows([rule('extension', 'png'), rule('domain', 'figma.com'), rule('extension', 'svg')], en);
    expect(rows.map(ruleText)).toEqual(['File type is png', 'URL domain is figma.com', 'File type is svg']);
    expect(rows.map((r) => r.ids.length)).toEqual([1, 1, 1]);
  });
});
