import { describe, expect, it } from 'vitest'
import { grillReply, parseGrill } from './grill'

const REPLY = `The grill starts below.

## Round 1

### Platform

❓ **Q1 - Windows 11 only**: Snare targets Windows 11 only, with the SDK floor already set.

➡️ Yes. Windows 11 only.

---

❓ **Q2 - HDR**: Should v1 assume HDR stays off?
Snare would log a warning.

➡️ Yes. HDR off is assumed
and detection only logs.

Q3 - No recommendation: just a question.
`

describe('parseGrill', () => {
  it('reads each question with its recommendation', () => {
    const qs = parseGrill(REPLY)
    expect(qs.map(q => q.id)).toEqual(['Q1', 'Q2'])
    expect(qs[0]).toMatchObject({ title: 'Windows 11 only', recommendation: 'Yes. Windows 11 only.' })
    expect(qs[1]!.body).toBe('Should v1 assume HDR stays off? Snare would log a warning.')
    expect(qs[1]!.recommendation).toBe('Yes. HDR off is assumed and detection only logs.')
  })
  it('finds nothing in an ordinary reply', () => {
    expect(parseGrill('All done. I changed two files.')).toEqual([])
    expect(parseGrill(null)).toEqual([])
  })
})

describe('grillReply', () => {
  it('writes accepted and typed answers and skips blanks', () => {
    const [a, b] = parseGrill(REPLY)
    const text = grillReply([{ question: a!, accept: true, text: '' }, { question: b!, accept: false, text: ' No, turn it on. ' }])
    expect(text).toBe('Q1 - Windows 11 only: Accepted. Yes. Windows 11 only.\nQ2 - HDR: No, turn it on.')
    expect(grillReply([{ question: a!, accept: false, text: ' ' }])).toBe('')
  })
})
