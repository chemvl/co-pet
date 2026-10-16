// Palette matching: accent- and case-insensitive token search with highlight ranges in the
// original label. `fold` follows the same rules as Rust `db::search::fold`.

const MARK = /\p{Mn}/gu;

/** NFD, drop nonspacing marks, `đ/Đ → d`, lowercase. */
export function fold(s: string): string {
  return s.normalize('NFD').replace(MARK, '').replace(/[đĐ]/g, 'd').toLowerCase();
}

/** The folded label plus, per folded UTF-16 unit, the `[start, end)` of its source character. */
function foldWithMap(label: string): { folded: string; from: number[]; to: number[] } {
  let folded = '';
  const from: number[] = [];
  const to: number[] = [];
  let i = 0;
  for (const ch of label) {
    const f = fold(ch);
    for (let k = 0; k < f.length; k++) {
      from.push(i);
      to.push(i + ch.length);
    }
    folded += f;
    i += ch.length;
  }
  return { folded, from, to };
}

/**
 * Every whitespace-separated folded query token must occur in the folded label. Returns the
 * matched `[start, end)` ranges in `label` indexes (sorted, merged), `[]` for an empty query,
 * or `null` when a token is missing.
 */
export function matchRanges(label: string, query: string): Array<[number, number]> | null {
  const tokens = fold(query).split(/\s+/).filter(Boolean);
  if (!tokens.length) return [];
  const { folded, from, to } = foldWithMap(label);
  const ranges: Array<[number, number]> = [];
  for (const token of tokens) {
    const at = folded.indexOf(token);
    if (at < 0) return null;
    ranges.push([from[at], to[at + token.length - 1]]);
  }
  ranges.sort((a, b) => a[0] - b[0]);
  const merged: Array<[number, number]> = [];
  for (const r of ranges) {
    const last = merged[merged.length - 1];
    if (last && r[0] <= last[1]) last[1] = Math.max(last[1], r[1]);
    else merged.push([r[0], r[1]]);
  }
  return merged;
}
