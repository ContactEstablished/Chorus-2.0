/**
 * The ids with `from` moved to where `target` is. Dropping on a row that's below takes the row's
 * place after it; dropping on a row above takes its place before it, so the dragged row always ends
 * up where it was dropped.
 */
export function moveTo(ids: number[], from: number, target: number): number[] {
  const i = ids.indexOf(from)
  const j = ids.indexOf(target)
  if (i < 0 || j < 0 || i === j) return ids
  const out = ids.filter((id) => id !== from)
  out.splice(j, 0, from)
  return out
}
