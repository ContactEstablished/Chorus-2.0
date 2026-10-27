import { describe, expect, it } from 'vitest'
import { tokenCount, usageText } from './labels'

const none = { input: null, cached: null, cacheWrite: null, output: null, reasoning: null, costUsd: null }

describe('usage text', () => {
  it('counts short', () => {
    expect([980, 41_234, 1_300_000].map(tokenCount)).toEqual(['980', '41.2k', '1.3M'])
  })
  it('adds cached tokens into the input and names them', () => {
    expect(usageText({ ...none, input: 3583, cached: 4224, output: 2, reasoning: 0 })).toBe('7.8k in (4.2k cached) · 2 out')
    expect(usageText({ ...none, input: 130, output: 765, reasoning: 43 })).toBe('130 in · 765 out · 43 reasoning')
  })
  it('says nothing when nothing is known', () => {
    expect(usageText(none)).toBeNull()
    expect(usageText(null)).toBeNull()
  })
})
