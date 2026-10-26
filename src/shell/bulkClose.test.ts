import { describe, expect, it } from 'vitest'
import type { SessionSummary } from '../ipc'
import { closeHeadline, eachLimited, pickable, pickedState, worthShowing, type CloseResult } from './bulkClose'

const s = (id: number, locked = false) => ({ id, locked }) as SessionSummary
const ok = (id: number, over: Partial<CloseResult['report'] & object> = {}): CloseResult => ({
  id,
  name: `S${id}`,
  report: { killed: 0, spared: 0, worktreeNote: null, ...over },
})
const bad = (id: number): CloseResult => ({ id, name: `S${id}`, error: 'refused' })

describe('picking', () => {
  it('leaves locked sessions out', () => {
    expect(pickable(s(1))).toBe(true)
    expect(pickable(s(2, true))).toBe(false)
  })

  it('says whether all, some or none of the pickable shown are picked', () => {
    const shown = [s(1), s(2), s(3, true)]
    expect(pickedState(shown, new Set())).toBe('none')
    expect(pickedState(shown, new Set([1]))).toBe('some')
    expect(pickedState(shown, new Set([1, 2]))).toBe('all')
    // A picked session that isn't shown doesn't count.
    expect(pickedState([s(1)], new Set([9]))).toBe('none')
    expect(pickedState([s(1, true)], new Set())).toBe('none')
  })
})

describe('eachLimited', () => {
  it('keeps order and never runs more than the limit at once', async () => {
    let running = 0
    let most = 0
    const out = await eachLimited([5, 1, 4, 2, 3, 0], 2, async (n) => {
      running++
      most = Math.max(most, running)
      await new Promise((r) => setTimeout(r, n))
      running--
      return n * 10
    })
    expect(out).toEqual([50, 10, 40, 20, 30, 0])
    expect(most).toBe(2)
  })

  it('does nothing for no items', async () => {
    expect(await eachLimited([], 4, async () => 1)).toEqual([])
  })
})

describe('outcome', () => {
  it('heads with how many closed', () => {
    expect(closeHeadline([ok(1)])).toBe('Closed 1 session.')
    expect(closeHeadline([ok(1), ok(2)])).toBe('Closed 2 sessions.')
    expect(closeHeadline([ok(1), bad(2), ok(3)])).toBe('Closed 2 of 3 sessions.')
    expect(closeHeadline([bad(1)])).toBe("Couldn't close the session.")
    expect(closeHeadline([bad(1), bad(2)])).toBe("Couldn't close any of the 2 sessions.")
  })

  it('is worth showing only when something needs reading', () => {
    expect(worthShowing([ok(1), ok(2, { killed: 3 })])).toBe(false)
    expect(worthShowing([ok(1), bad(2)])).toBe(true)
    expect(worthShowing([ok(1, { worktreeNote: 'Removed the worktree.' })])).toBe(true)
    expect(worthShowing([ok(1, { spared: 1 })])).toBe(true)
  })
})
