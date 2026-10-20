/** One option of a ButtonGroup. A disabled option says why in `title`. */
export interface Choice<T> {
  value: T
  label: string
  title?: string
  disabled?: boolean
  /** Shown beside the chosen option in a `radios` group (the argument it adds). */
  hint?: string
}

/**
 * Cleans a tag as the back end does (`chorus_core::tags`): trimmed, a leading `#` dropped, inner
 * spaces collapsed, lowercased. Empty when nothing usable is left.
 */
export function cleanTag(text: string): string {
  return text.trim().replace(/^#+/, '').trim().split(/\s+/).filter(Boolean).join(' ').toLowerCase().replace(/,/g, '').slice(0, MAX_TAG_CHARS)
}

export const MAX_TAG_CHARS = 24
export const MAX_TAGS = 8

/**
 * Suggestions for what's typed: names that start with it first, then names that contain it, in
 * the order given (most used first). Without a query, the first `limit`.
 */
export function suggest(names: string[], query: string, exclude: string[], limit = 8): string[] {
  const q = query.trim().toLowerCase()
  const free = names.filter((n) => !exclude.includes(n))
  if (!q) return free.slice(0, limit)
  const starts = free.filter((n) => n.toLowerCase().startsWith(q))
  const contains = free.filter((n) => !n.toLowerCase().startsWith(q) && n.toLowerCase().includes(q))
  return [...starts, ...contains].slice(0, limit)
}
