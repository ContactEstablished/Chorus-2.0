/** Windows image file paths an agent printed in a message, in order of appearance, without repeats. */
const EXT = '(?:png|jpe?g|webp|gif)'
// A quoted path may contain spaces; a bare one may not, so prose after a drive letter is never swallowed.
const QUOTED = new RegExp('[`"\']([A-Za-z]:[\\\\/][^`"\'\\r\\n<>|?*]+?\\.' + EXT + ')[`"\']', 'gi')
const BARE = new RegExp('(?<![\\w`"\'/\\\\])[A-Za-z]:[\\\\/][^\\s`"\'<>|?*]+?\\.' + EXT + '(?![\\w\\\\/])', 'gi')
export const MAX_LINKED_IMAGES = 6

export function extractImagePaths(text: string): string[] {
  const found: { at: number; path: string }[] = []
  for (const m of text.matchAll(QUOTED)) found.push({ at: m.index, path: m[1]! })
  for (const m of text.matchAll(BARE)) found.push({ at: m.index, path: m[0] })
  const seen = new Set<string>()
  const out: string[] = []
  for (const { path } of found.sort((a, b) => a.at - b.at)) {
    if (path.split(/[\\/]/).includes('..')) continue
    const key = path.replace(/\//g, '\\').toLowerCase()
    if (seen.has(key)) continue
    seen.add(key)
    out.push(path)
    if (out.length === MAX_LINKED_IMAGES) break
  }
  return out
}
