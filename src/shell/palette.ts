/** The command palette's items and how a query picks them (Ctrl+K, and Ctrl+G for projects). */

export interface PaletteItem {
  id: string
  group: string
  label: string
  /** Shown beside the label, and searched. */
  hint?: string
  /** Searched, not shown. */
  keywords?: string
  run: () => void
}

/**
 * The items that match every word of `query`, best first: label starts with the query, then a
 * word in the label starts with it, then anywhere. Ties keep their given order. Empty query: all.
 */
export function filterItems(items: PaletteItem[], query: string, limit = 50): PaletteItem[] {
  const q = query.trim().toLowerCase()
  if (!q) return items.slice(0, limit)
  const words = q.split(/\s+/)
  const scored: { item: PaletteItem; score: number; i: number }[] = []
  items.forEach((item, i) => {
    const label = item.label.toLowerCase()
    const hay = `${label}\n${item.hint ?? ''}\n${item.keywords ?? ''}\n${item.group}`.toLowerCase()
    if (!words.every((w) => hay.includes(w))) return
    const score = label.startsWith(q) ? 0 : label.split(/[\s·/-]+/).some((w) => w.startsWith(words[0]!)) ? 1 : 2
    scored.push({ item, score, i })
  })
  return scored
    .sort((a, b) => a.score - b.score || a.i - b.i)
    .slice(0, limit)
    .map((s) => s.item)
}

/** Moves a selection by `by` through `count` items, wrapping. */
export function moveSelection(current: number, by: number, count: number): number {
  if (count === 0) return 0
  return (((current + by) % count) + count) % count
}
