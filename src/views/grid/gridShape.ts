/** The shape a terminal reads best at: about 100 columns by 30 rows of a monospace font. */
export const TILE_ASPECT = 1.6
/** Below this a tile is too small to read, so the grid scrolls instead of shrinking further. */
export const MIN_TILE = { width: 300, height: 180 }

export interface GridShape {
  cols: number
  rows: number
  /** True when the tiles don't fit at MIN_TILE, so rows keep MIN_TILE's height and the grid scrolls. */
  scroll: boolean
}

/**
 * How to lay `n` tiles in a `width` × `height` box with `gap` between them: the column count whose
 * tiles hold the largest terminal-shaped area, so the grid fills the box without scrolling. When even
 * the best fit is smaller than MIN_TILE, as many MIN_TILE-wide columns as fit, scrolling.
 */
export function gridShape(n: number, width: number, height: number, gap: number): GridShape {
  if (n <= 0 || width <= 0 || height <= 0) return { cols: 1, rows: Math.max(n, 0), scroll: false }
  let best = { cols: 1, rows: n, w: 0, h: 0, area: -1 }
  for (let cols = 1; cols <= n; cols++) {
    const rows = Math.ceil(n / cols)
    // A column count that leaves a whole row empty is never better than one fewer column.
    if (cols > 1 && Math.ceil(n / (cols - 1)) === rows) continue
    const w = (width - gap * (cols - 1)) / cols
    const h = (height - gap * (rows - 1)) / rows
    const fitW = Math.min(w, h * TILE_ASPECT)
    const area = fitW * (fitW / TILE_ASPECT)
    if (area > best.area) best = { cols, rows, w, h, area }
  }
  if (best.w >= MIN_TILE.width && best.h >= MIN_TILE.height) return { cols: best.cols, rows: best.rows, scroll: false }
  const cols = Math.max(1, Math.min(n, Math.floor((width + gap) / (MIN_TILE.width + gap))))
  return { cols, rows: Math.ceil(n / cols), scroll: true }
}
