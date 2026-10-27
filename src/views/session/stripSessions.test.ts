import { describe, expect, it } from 'vitest'
import type { Attention, SessionSummary } from '../../ipc'
import { stripSessions } from './stripSessions'

function s(id: number, projectId: number | null, attention: Attention = 'ready'): SessionSummary {
  return { id, projectId, attention, latestAtSecs: 0, createdAtSecs: id } as SessionSummary
}

describe('stripSessions', () => {
  const all = [s(1, 7, 'done'), s(2, 7, 'approval'), s(3, 8), s(4, null), s(5, null)]

  it('keeps the current session\'s project only, most urgent first', () => {
    expect(stripSessions(all, 1).map((x) => x.id)).toEqual([2, 1])
    expect(stripSessions(all, 3).map((x) => x.id)).toEqual([3])
  })
  it('groups sessions in no project together', () => {
    expect(stripSessions(all, 4).map((x) => x.id).sort()).toEqual([4, 5])
  })
  it('offers nothing for an unknown session', () => {
    expect(stripSessions(all, 99)).toEqual([])
  })
})
