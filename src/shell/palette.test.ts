import { describe, expect, it } from 'vitest'
import { filterItems, moveSelection, type PaletteItem } from './palette'

const item = (id: string, label: string, extra: Partial<PaletteItem> = {}): PaletteItem => ({ id, group: 'g', label, run: () => {}, ...extra })

describe('command palette', () => {
  const items = [
    item('a', 'Open settings', { keywords: 'preferences' }),
    item('b', 'Billing dashboard rewrite', { hint: 'Claude Code · Ledger' }),
    item('c', 'Rate limiter middleware', { hint: 'Claude Code · Ledger' }),
    item('d', 'Settings: Projects'),
  ]

  it('matches every word anywhere, best match first', () => {
    expect(filterItems(items, 'settings').map((i) => i.id)).toEqual(['d', 'a'])
    expect(filterItems(items, 'ledger').map((i) => i.id)).toEqual(['b', 'c'])
    expect(filterItems(items, 'claude rate').map((i) => i.id)).toEqual(['c'])
    expect(filterItems(items, 'prefer').map((i) => i.id)).toEqual(['a'])
    expect(filterItems(items, 'nothing here')).toEqual([])
  })

  it('returns everything for an empty query, up to the limit', () => {
    expect(filterItems(items, '  ').length).toBe(4)
    expect(filterItems(items, '', 2).length).toBe(2)
  })

  it('wraps the selection', () => {
    expect(moveSelection(0, -1, 3)).toBe(2)
    expect(moveSelection(2, 1, 3)).toBe(0)
    expect(moveSelection(0, 1, 0)).toBe(0)
  })
})
