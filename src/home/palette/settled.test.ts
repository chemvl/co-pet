import { describe, expect, it } from 'vitest';
import { resultsSettled } from './settled';

const done = { isPending: false, isError: false };
const pending = { isPending: true, isError: false };

describe('resultsSettled', () => {
  it('is false while the query is debouncing', () => {
    expect(resultsSettled('duong', 'duon', [done, done])).toBe(false);
  });
  it('is false while a result query is pending', () => {
    expect(resultsSettled('duong', 'duong', [done, pending])).toBe(false);
  });
  it('is true once every query has settled', () => {
    expect(resultsSettled('duong', 'duong', [done, done])).toBe(true);
    expect(resultsSettled('', '', [pending, pending])).toBe(true);
  });
});
