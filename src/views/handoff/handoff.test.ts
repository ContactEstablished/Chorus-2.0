import { describe, expect, it } from 'vitest'
import type { HandoffEntry } from '../../ipc'
import { authorLabel, earlierLabel, entryRows, ignoreLabel, updatedLabel } from './handoff'

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
