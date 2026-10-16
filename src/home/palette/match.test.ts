import { describe, expect, it } from 'vitest';
import { fold, matchRanges } from './match';

const pick = (label: string, ranges: Array<[number, number]>) => ranges.map(([a, b]) => label.slice(a, b));

describe('fold', () => {
  it('matches the Rust rules', () => {
    expect(fold('Đường đến Việt Nam')).toBe('duong den viet nam');
  });

  it('lowercases per code point, like Rust char::to_lowercase', () => {
    expect(fold('ΟΔΟΣ')).toBe('οδοσ');
    expect(matchRanges('ΟΔΟΣ', 'οδοσ')).toEqual([[0, 4]]);
    expect(matchRanges('ΟΔΟΣ', 'οδος')).toBeNull();
  });
});

describe('matchRanges', () => {
  it('matches accented labels with plain queries', () => {
    expect(matchRanges('Xuất', 'xuat')).toEqual([[0, 4]]);
    expect(matchRanges('đường', 'duong')).toEqual([[0, 5]]);
  });

  it('highlights the original accented characters', () => {
    const label = 'Xuất sang Notion';
    expect(pick(label, matchRanges(label, 'uat notion')!)).toEqual(['uất', 'Notion']);
  });

  it('maps through decomposed input', () => {
    const label = 'Vie\u0302\u0323t Nam';
    expect(pick(label, matchRanges(label, 'viet')!)).toEqual(['Vie\u0302\u0323t']);
  });

  it('never splits a grapheme at a lone combining mark', () => {
    expect(matchRanges('e\u0302x', 'e')).toEqual([[0, 2]]);
    expect(matchRanges('xe\u0302\u0323', 'e')).toEqual([[1, 4]]);
  });

  it('merges overlapping tokens and accepts an empty query', () => {
    expect(matchRanges('Go to Tasks', 'tas task')).toEqual([[6, 10]]);
    expect(matchRanges('Go to Tasks', '  ')).toEqual([]);
  });

  it('returns null when a token is missing', () => {
    expect(matchRanges('Go to Inbox', 'go tasks')).toBeNull();
  });
});
