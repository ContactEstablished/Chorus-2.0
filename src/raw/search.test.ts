import { describe, expect, it } from 'vitest'
import { matchCount } from './search'

describe('Raw search count', () => {
  it('says nothing before a search', () => {
    expect(matchCount('', { index: 0, count: 3 })).toBe('')
    expect(matchCount('hi', null)).toBe('')
  })

  it('counts from one, says when nothing matched, and when the limit was passed', () => {
    expect(matchCount('hi', { index: 0, count: 3 })).toBe('1 of 3')
    expect(matchCount('hi', { index: 2, count: 3 })).toBe('3 of 3')
    expect(matchCount('hi', { index: -1, count: 0 })).toBe('No matches')
    expect(matchCount('hi', { index: -1, count: 1000 })).toBe('1000+ matches')
  })
})
