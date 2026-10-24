import { describe, expect, it } from 'vitest'
import type { Attention, Project, SessionSummary } from '../ipc'
import { ago, counts, groupSessions, headline, matchesFilter, shortDuration, shortPath } from './status'

function s(id: number, attention: Attention, over: Partial<SessionSummary> = {}): SessionSummary {
  return { id, attention, projectId: null, latestAtSecs: 0, runStartedAtSecs: null, createdAtSecs: id, ...over } as SessionSummary
}

const project = (id: number, name: string): Project => ({ id, name, folder: null, colour: '#f472b6', state: 'active' })

describe('overview status', () => {
  const list = [s(1, 'done'), s(2, 'working'), s(3, 'failed'), s(4, 'approval'), s(5, 'running'), s(6, 'input')]

  it('counts and headlines as the design says', () => {
    const c = counts(list)
    expect(c).toEqual({ total: 6, needs: 3, working: 1, ready: 0, failed: 1 })
    expect(headline(c)).toBe('3 sessions need you · 1 working · 6 total')
    expect(headline(counts([s(1, 'input')]))).toBe('1 session needs you · 1 total')
  })

  it('groups by status, most urgent first, leaving out empty groups', () => {
    const g = groupSessions(list, 'status', [])
    expect(g.map((x) => x.key)).toEqual(['needs', 'flight', 'finished'])
    expect(g[0]!.sessions.map((x) => x.id)).toEqual([4, 6, 3])
    expect(g[1]!.sessions.map((x) => x.id)).toEqual([2, 5])
    expect(groupSessions([s(1, 'done')], 'status', []).map((x) => x.key)).toEqual(['finished'])
  })

  it('puts older waiting episodes ahead of newer approvals, with unknown ages last', () => {
    const items = [s(1, 'approval', { needsYouSince: 3000 }), s(2, 'failed', { needsYouSince: 1000 }),
      s(3, 'input', { needsYouSince: 2000 }), s(4, 'approval', { needsYouSince: null })]
    for (const grouping of ['status', 'none', 'project'] as const) {
      expect(groupSessions(items, grouping, [])[0]!.sessions.map((x) => x.id)).toEqual([2, 3, 1, 4])
    }
  })

  it('groups by project in rail order, then sessions in none', () => {
    const g = groupSessions([s(1, 'done', { projectId: 9 }), s(2, 'ready'), s(3, 'input', { projectId: 7 })], 'project', [project(9, 'A'), project(7, 'B')])
    expect(g.map((x) => [x.title, x.sessions.map((y) => y.id)])).toEqual([
      ['A', [1]],
      ['B', [3]],
      ['No project', [2]],
    ])
  })

  it('filters by status pill', () => {
    expect(list.filter((x) => matchesFilter(x, 'needs')).map((x) => x.id)).toEqual([3, 4, 6])
    expect(list.filter((x) => matchesFilter(x, 'failed')).map((x) => x.id)).toEqual([3])
  })

  it('writes short durations and paths', () => {
    expect(shortDuration(12)).toBe('12s')
    expect(shortDuration(23 * 60)).toBe('23m')
    expect(shortDuration(2 * 3600 + 5 * 60)).toBe('2h 05m')
    expect(ago(100, 105)).toBe('just now')
    expect(ago(0, 105)).toBe('never')
    expect(shortPath('C:\\Users\\me\\work\\ledger', 'C:\\Users\\me')).toBe('~/work/ledger')
    expect(shortPath('D:\\x', 'C:\\Users\\me')).toBe('D:/x')
  })
})
