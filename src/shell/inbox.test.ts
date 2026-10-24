import { describe, expect, it } from 'vitest'
import { ageLabel, attentionTier, inboxKey, moveSelection, nextAfterReturn, waitingSeconds } from './inbox'

describe('attention inbox', () => {
  it('uses milliseconds, clamps clock skew and leaves missing evidence unknown', () => {
    expect(waitingSeconds(10_000, 12)).toBe(2)
    expect(waitingSeconds(20_000, 12)).toBe(0)
    expect(waitingSeconds(null, 12)).toBeNull()
    expect(waitingSeconds(undefined, 12)).toBeNull()
    expect(waitingSeconds(NaN, 12)).toBeNull()
  })
  it('keeps escalation boundaries and readable long ages', () => {
    expect([0, 29, 30, 299, 300, 1199, 1200].map(attentionTier)).toEqual(['calm', 'calm', 'pulse', 'pulse', 'urgent', 'urgent', 'stale'])
    expect([0, 59, 60, 252, 3600, 86400].map(ageLabel)).toEqual(['0s', '59s', '1m 0s', '4m 12s', '1h 0m', '1d 0h'])
  })
  it('navigates within bounds and handles an item disappearing', () => {
    expect(moveSelection([], null, 1)).toBeNull()
    expect(moveSelection([1, 2], null, 1)).toBe(1)
    expect(moveSelection([1, 2], null, -1)).toBe(2)
    expect(moveSelection([1, 2], 2, 1)).toBe(2)
    expect(moveSelection([1, 2], 99, 1)).toBe(1)
    expect(nextAfterReturn([1, 2, 3], 2, [1, 3, 4])).toBe(3)
    expect(nextAfterReturn([1, 2, 3], 3, [1, 3, 4])).toBe(1)
    expect(nextAfterReturn([1], 1, [])).toBeNull()
    expect(nextAfterReturn([1], 1, [4])).toBe(4)
  })
  it('never steals typing, modifiers, composition or already handled keys', () => {
    const event = { key: 'j', ctrlKey: false, altKey: false, metaKey: false, shiftKey: false, isComposing: false, defaultPrevented: false }
    expect(inboxKey(event, false)).toBe('next')
    expect(inboxKey(event, true)).toBeNull()
    for (const flag of ['ctrlKey', 'altKey', 'metaKey', 'shiftKey', 'isComposing', 'defaultPrevented']) {
      expect(inboxKey({ ...event, [flag]: true }, false)).toBeNull()
    }
    expect(inboxKey({ ...event, key: 'k' }, false)).toBe('previous')
    expect(inboxKey({ ...event, key: 'Enter' }, false)).toBe('open')
    expect(inboxKey({ ...event, key: 'Escape' }, false)).toBe('back')
    expect(inboxKey({ ...event, key: 'x' }, false)).toBeNull()
  })
})
