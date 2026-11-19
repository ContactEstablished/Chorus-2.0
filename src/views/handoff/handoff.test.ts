import { describe, expect, it } from 'vitest'
import type { HandoffEntry } from '../../ipc'
import { activityLabel, authorLabel, branchLine, branchState, canDelete, deleteQuestion, earlierLabel, entryRows, fileStat, ignoreLabel, moreLabel, returnTitle, updatedLabel } from './handoff'

const NOW = Date.parse('2026-11-18T14:05:00Z')

function entry(over: Partial<HandoffEntry> = {}): HandoffEntry {
  return { heading: '2026-11-18 14:05 · Chorus · latest work', author: 'chorus', latestWork: true, done: null, state: null, open: null, next: null, other: [], ...over }
}

describe('hand-off note wording', () => {
  it('says how long ago the note was written', () => {
    expect(updatedLabel(null, NOW)).toBe('Not written yet')
    expect(updatedLabel('2026-11-18T14:04:40Z', NOW)).toBe('Updated just now')
    expect(updatedLabel('2026-11-18T14:02:00Z', NOW)).toBe('Updated 3 min ago')
    expect(updatedLabel('2026-11-18T11:05:00Z', NOW)).toBe('Updated 3 h ago')
    expect(updatedLabel('2026-11-10T11:05:00Z', NOW)).toMatch(/^Updated /)
    expect(updatedLabel('soon', NOW)).toBe('Updated soon')
  })
  it('says whether git ignores it', () => {
    expect(ignoreLabel('ignored')).toBe('ignored by git')
    expect(ignoreLabel('not-ignored')).toBe('not ignored by git yet')
    expect(ignoreLabel('not-a-repo')).toBe('not a git repository')
    expect(ignoreLabel(null)).toBeNull()
  })
  it('lists the labelled lines that exist, in order', () => {
    expect(entryRows(entry({ next: 'N', done: 'D' }))).toEqual([
      { label: 'Done', value: 'D' },
      { label: 'Next', value: 'N' },
    ])
    expect(entryRows(entry())).toEqual([])
  })
  it('names the author and the earlier entries', () => {
    expect(authorLabel(entry())).toBe('Chorus')
    expect(authorLabel(entry({ author: 'mausbot' }))).toBe('MausBot')
    expect(authorLabel(entry({ author: 'other' }))).toBe('')
    expect(earlierLabel(1, false)).toBe('Show 1 earlier entry')
    expect(earlierLabel(3, false)).toBe('Show 3 earlier entries')
    expect(earlierLabel(3, true)).toBe('Hide earlier entries')
  })
})

describe('baton wording', () => {
  it('says what MausBot is doing', () => {
    expect(activityLabel(null, NOW)).toBe('Chorus hasn’t asked MausBot yet')
    expect(activityLabel({ found: false, busy: false, updatedAtMs: null, problem: 'down' }, NOW)).toBe('MausBot isn’t answering')
    expect(activityLabel({ found: false, busy: false, updatedAtMs: null, problem: null }, NOW)).toMatch(/can’t see the conversation/)
    expect(activityLabel({ found: true, busy: true, updatedAtMs: NOW, problem: null }, NOW)).toBe('MausBot is working')
    expect(activityLabel({ found: true, busy: false, updatedAtMs: NOW - 5 * 60_000, problem: null }, NOW)).toBe('MausBot last worked 5 min ago')
    expect(activityLabel({ found: true, busy: false, updatedAtMs: NOW - 3 * 3_600_000, problem: null }, NOW)).toBe('MausBot last worked 3 h ago')
  })
  it('says who handed back and where the branch stands', () => {
    expect(returnTitle('Atlas', 'mausbot')).toBe('MausBot handed Atlas back')
    expect(returnTitle('Atlas', 'chorus')).toBe('You took Atlas back')
    expect(branchState({ merged: false, ahead: 4, base: 'main @ 1a2b3c4' })).toBe('Not merged yet: 4 commits ahead of main')
    expect(branchState({ merged: false, ahead: 1, base: 'main @ 1a2b3c4' })).toBe('Not merged yet: 1 commit ahead of main')
    expect(branchState({ merged: true, ahead: 2, base: 'main @ 1a2b3c4' })).toBe('Merged into main')
    expect(branchState({ merged: false, ahead: 0, base: 'detached @ 1a2b3c4' })).toBe('No commits ahead of detached')
  })
  it('counts files and what is left out', () => {
    expect(fileStat({ path: 'a', added: 3, removed: 1 })).toBe('+3 −1')
    expect(fileStat({ path: 'a', added: null, removed: null })).toBe('binary')
    expect(moreLabel(0, 'file', 'files')).toBeNull()
    expect(moreLabel(1, 'file', 'files')).toBe('and 1 more file')
    expect(moreLabel(3, 'commit', 'commits')).toBe('and 3 more commits')
  })
})

describe('branch wording', () => {
  it('says how long a branch has sat and where it stands', () => {
    expect(branchLine({ days: 45, merged: true, ahead: 0, stale: true }, 'main')).toBe('45 days unused · merged into main')
    expect(branchLine({ days: 60, merged: false, ahead: 2, stale: true }, 'main')).toBe('60 days unused · 2 commits not merged')
    expect(branchLine({ days: 3, merged: false, ahead: 1, stale: false }, null)).toBe('1 commit not merged')
    expect(branchLine({ days: 40, merged: false, ahead: null, stale: true }, 'main')).toBe('40 days unused · not merged, and Chorus couldn’t count its commits')
  })
  it('never offers Delete for a count it doesn’t know or a hidden name', () => {
    expect(canDelete({ merged: true, ahead: 0, redacted: false })).toBe(true)
    expect(canDelete({ merged: false, ahead: 2, redacted: false })).toBe(true)
    expect(canDelete({ merged: false, ahead: null, redacted: false })).toBe(false)
    expect(canDelete({ merged: true, ahead: 0, redacted: true })).toBe(false)
  })
  it('asks before unmerged commits go', () => {
    expect(deleteQuestion({ name: 'old-open', ahead: 2 }, 'main')).toBe('2 commits on old-open aren’t merged into main. Delete them for good?')
    expect(deleteQuestion({ name: 'x', ahead: 1 }, null)).toBe('1 commit on x isn’t merged into the default branch. Delete it for good?')
  })
})
