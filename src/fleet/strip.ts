import type { SessionSummary } from '../ipc'
import { ageLabel, attentionTier, waitingSeconds, type AttentionTier } from '../shell/inbox'
import { needsYou } from '../shell/status'

export interface Chip {
  id: number
  name: string
  tier: AttentionTier | null
  /** "Waiting 4m 12s", or that the start is unknown. */
  waiting: string
}

export interface StripView {
  needs: number
  working: number
  ready: number
  chips: Chip[]
  /** Sessions needing you that didn't fit. */
  more: number
}

/** The strip's content: counts, and the sessions that need you, longest waiting first. */
export function stripView(list: SessionSummary[], nowSeconds: number, maxChips = 3): StripView {
  const open = list.filter((s) => s.status !== 'closed')
  const waiting = open
    .filter((s) => needsYou(s.attention))
    .sort((a, b) => (a.needsYouSince ?? Infinity) - (b.needsYouSince ?? Infinity) || a.id - b.id)
  const chips = waiting.slice(0, maxChips).map((s) => {
    const secs = waitingSeconds(s.needsYouSince, nowSeconds)
    return {
      id: s.id,
      name: s.name,
      tier: secs === null ? null : attentionTier(secs),
      waiting: secs === null ? 'Waiting · start unknown' : `Waiting ${ageLabel(secs)}`,
    }
  })
  return {
    needs: waiting.length,
    working: open.filter((s) => s.attention === 'working').length,
    ready: open.filter((s) => s.attention === 'ready').length,
    chips,
    more: Math.max(0, waiting.length - chips.length),
  }
}
