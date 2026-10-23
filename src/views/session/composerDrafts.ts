import { reactive } from 'vue'

// View changes keep unsent drafts in memory. Only explicit sends/queueing persist prompts.
const drafts = reactive(new Map<number, { text: string; requestId: string | null }>())
export function composerDraft(sessionId: number): { text: string; requestId: string | null } {
  if (!drafts.has(sessionId)) drafts.set(sessionId, { text: '', requestId: null })
  return drafts.get(sessionId)!
}
export function setComposerDraft(sessionId: number, text: string): void {
  const draft = composerDraft(sessionId)
  draft.text = text
  draft.requestId = null
}

export function stageComposerSuggestion(sessionId: number, text: string): void {
  const current = composerDraft(sessionId).text
  setComposerDraft(sessionId, current.trim() ? `${current}\n\n${text}` : text)
}
