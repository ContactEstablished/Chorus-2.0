/** Bound a displayed path while keeping its beginning and final folder visible. Full path is a tooltip. */
export function compactPath(path: string, maxLength = 44): string {
  const chars = Array.from(path.replace(/\\/g, '/'))
  const limit = Math.max(8, Math.floor(maxLength))
  if (chars.length <= limit) return chars.join('')
  const end = chars.slice(chars.lastIndexOf('/'))
  const prefixLength = limit - 3 - end.length
  if (prefixLength >= 3) return chars.slice(0, prefixLength).join('') + '...' + end.join('')
  // A folder name may itself exceed the budget. Keep the root and the end of that name.
  const head = Math.min(8, Math.floor((limit - 3) / 3))
  return chars.slice(0, head).join('') + '...' + chars.slice(-(limit - head - 3)).join('')
}
