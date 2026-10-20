import { describe, expect, it } from 'vitest'
import { MAX_TAG_CHARS, cleanTag, suggest } from './choice'

describe('tags and suggestions', () => {
  it('cleans a tag the way the back end does', () => {
    expect(cleanTag('  #Bug   Fix ')).toBe('bug fix')
    expect(cleanTag(' # ')).toBe('')
    expect(cleanTag('a,b')).toBe('ab')
    expect(cleanTag('x'.repeat(40))).toHaveLength(MAX_TAG_CHARS)
  })

  it('offers names that start with the query first, then ones that contain it', () => {
    const names = ['review', 'spike', 'prod-fix', 'fix', 'bugfix']
    expect(suggest(names, 'fi', [])).toEqual(['fix', 'prod-fix', 'bugfix'])
    expect(suggest(names, '', ['spike'], 2)).toEqual(['review', 'prod-fix'])
    expect(suggest(names, 'FIX', ['fix'])).toEqual(['prod-fix', 'bugfix'])
  })
})
