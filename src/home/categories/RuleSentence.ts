// The Categories screen's rule rows: one sentence per rule, with consecutive `extension` rules
// grouped into one "File type is any of …" row whose count is the sum and whose delete removes all.
import type { I18nKey, TFunction } from '../../shared/i18n';
import type { RuleView } from '../../shared/types';

export interface RuleRow {
  /** Every rule the row stands for (several for a grouped file-type row). */
  ids: string[];
  /** The sentence around the value: `before` + value + `after`. */
  before: string;
  value: string;
  after: string;
  /** The value is user text (domain, extension, app, pattern), shown in mono. */
  mono: boolean;
  matchCount: number;
}

const VALUE = '\u0000';

function row(t: TFunction, key: I18nKey, value: string, mono: boolean, rules: RuleView[]): RuleRow {
  const [before, after = ''] = t(key, { p: VALUE }).split(VALUE);
  return {
    ids: rules.map((r) => r.id),
    before,
    value,
    after,
    mono,
    matchCount: rules.reduce((n, r) => n + r.matchCount, 0),
  };
}

export function ruleRows(rules: readonly RuleView[], t: TFunction): RuleRow[] {
  const rows: RuleRow[] = [];
  for (let i = 0; i < rules.length; i++) {
    const r = rules[i];
    switch (r.kind) {
      case 'extension': {
        const group = [r];
        while (rules[i + 1]?.kind === 'extension') group.push(rules[++i]);
        rows.push(
          group.length === 1
            ? row(t, 'rules.extension', r.pattern, true, group)
            : row(t, 'rules.extensions', group.map((g) => g.pattern).join(', '), true, group),
        );
        break;
      }
      case 'domain':
        rows.push(row(t, 'rules.domain', r.pattern, true, [r]));
        break;
      case 'item_kind':
        rows.push(row(t, 'rules.itemKind', t(`kind.${r.pattern}` as I18nKey), false, [r]));
        break;
      case 'source_kind':
        rows.push(row(t, 'rules.sourceKind', t(`home.source.${r.pattern}` as I18nKey), false, [r]));
        break;
      case 'source_app':
        rows.push(row(t, 'rules.sourceApp', r.pattern, true, [r]));
        break;
      case 'regex':
        rows.push(row(t, 'rules.regex', r.pattern, true, [r]));
        break;
    }
  }
  return rows;
}

/** The row as plain text (tests, accessible names). */
export function ruleText(r: RuleRow): string {
  return r.before + r.value + r.after;
}
