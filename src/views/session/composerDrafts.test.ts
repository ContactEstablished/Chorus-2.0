import { describe, expect, it } from 'vitest'
import { composerDraft, setComposerDraft, stageComposerSuggestion } from './composerDrafts'

describe('composer draft staging', () => {
  it('preserves an existing draft and only stages a suggestion', () => {
    setComposerDraft(81, 'My unsent instruction')
    composerDraft(81).requestId = 'previous-attempt'
    stageComposerSuggestion(81, 'Review the changes')
    expect(composerDraft(81)).toEqual({ text: 'My unsent instruction\n\nReview the changes', requestId: null })
    expect(composerDraft(82).text).toBe('')
  })
  it('retains a draft across callers and invalidates an old request on edits', () => {
    setComposerDraft(83, 'Persist across view switches')
    const first = composerDraft(83)
    first.requestId = 'stable-until-edit'
    expect(composerDraft(83)).toBe(first)
    setComposerDraft(83, 'Updated')
    expect(composerDraft(83).requestId).toBeNull()
  })
})
