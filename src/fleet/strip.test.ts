import { describe, expect, it } from 'vitest'
import type { Attention, SessionSummary } from '../ipc'
import { stripView } from './strip'

function s(id: number, attention: Attention, over: Partial<SessionSummary> = {}): SessionSummary {
  return { id, name: `S${id}`, attention, status: 'running', needsYouSince: null, ...over } as SessionSummary
}

describe('fleet strip', () => {
  it('counts open sessions and lists the longest waiting first', () => {
    const v = stripView(
      [
        s(1, 'approval', { needsYouSince: 900_000 }),
        s(2, 'input', { needsYouSince: 100_000 }),
        s(3, 'failed'),
        s(4, 'working'),
        s(5, 'ready'),
        s(6, 'failed', { status: 'closed' }),
      ],
      1000,
    )
    expect(v).toMatchObject({ needs: 3, working: 1, ready: 1, more: 0 })
    expect(v.chips.map((c) => c.id)).toEqual([2, 1, 3])
    expect(v.chips[0]).toMatchObject({ tier: 'urgent', waiting: 'Waiting 15m 0s' })
    expect(v.chips[1]).toMatchObject({ tier: 'pulse', waiting: 'Waiting 1m 40s' })
    expect(v.chips[2]).toMatchObject({ tier: null, waiting: 'Waiting · start unknown' })
  })

  it('caps the chips and says how many more wait', () => {
    const list = [1, 2, 3, 4, 5].map((id) => s(id, 'input', { needsYouSince: id * 1000 }))
    const v = stripView(list, 100, 3)
    expect(v.chips.map((c) => c.id)).toEqual([1, 2, 3])
    expect(v.more).toBe(2)
  })

  it('is empty when nothing needs you', () => {
    expect(stripView([s(1, 'working')], 0)).toEqual({ needs: 0, working: 1, ready: 0, chips: [], more: 0 })
  })
})
