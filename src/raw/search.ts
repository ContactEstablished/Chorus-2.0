/** The Raw search bar's match count, in words: "3 of 12", "No matches", or "1000+ matches" past the
 * highlight limit, where the addon reports no active index. Empty before a search. */
export function matchCount(text: string, results: { index: number; count: number } | null): string {
  if (!text || !results) return ''
  if (!results.count) return 'No matches'
  return results.index < 0 ? `${results.count}+ matches` : `${results.index + 1} of ${results.count}`
}
