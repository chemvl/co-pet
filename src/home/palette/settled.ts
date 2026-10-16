// Whether the palette's item/task results reflect the typed query, so "no matches" is real.
export function resultsSettled(query: string, debounced: string, queries: ReadonlyArray<{ isPending: boolean; isError: boolean }>): boolean {
  if (query !== debounced) return false;
  if (query === '') return true;
  return queries.every((q) => !q.isPending || q.isError);
}
