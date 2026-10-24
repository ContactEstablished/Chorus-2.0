/** Waiting presentation and selection. Times are Unix milliseconds from persisted agent facts. */
export type AttentionTier = 'calm' | 'pulse' | 'urgent' | 'stale'

export function waitingSeconds(since: number | null | undefined, nowSeconds: number): number | null {
  return since == null || !Number.isFinite(since) ? null : Math.max(0, Math.floor(nowSeconds - since / 1000))
}

export function attentionTier(seconds: number): AttentionTier {
  return seconds < 30 ? 'calm' : seconds < 300 ? 'pulse' : seconds < 1200 ? 'urgent' : 'stale'
}

export function ageLabel(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds))
  if (s < 60) return `${s}s`
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`
  if (s < 86400) return `${Math.floor(s / 3600)}h ${Math.floor(s / 60) % 60}m`
  return `${Math.floor(s / 86400)}d ${Math.floor(s / 3600) % 24}h`
}

export function moveSelection(ids: number[], selected: number | null, direction: 1 | -1): number | null {
  if (!ids.length) return null
  const index = selected === null ? -1 : ids.indexOf(selected)
  if (index < 0) return direction === 1 ? ids[0]! : ids.at(-1)!
  return ids[Math.max(0, Math.min(ids.length - 1, index + direction))]!
}

/** Prefer the next surviving item in the reviewed order, including when the opened item vanished. */
export function nextAfterReturn(before: number[], opened: number, current: number[]): number | null {
  const index = before.indexOf(opened)
  const after = [...before.slice(index + 1), ...before.slice(0, Math.max(0, index))]
  return after.find((id) => current.includes(id)) ?? current.find((id) => id !== opened) ?? current[0] ?? null
}

/** A shortcut never consumes typing, IME composition, terminal input or modified keys. */
export function inboxKey(e: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'altKey' | 'metaKey' | 'shiftKey' | 'isComposing' | 'defaultPrevented'>,
  protectedTarget: boolean): 'next' | 'previous' | 'open' | 'back' | null {
  if (protectedTarget || e.defaultPrevented || e.isComposing || e.ctrlKey || e.altKey || e.metaKey || e.shiftKey) return null
  return e.key === 'j' ? 'next' : e.key === 'k' ? 'previous' : e.key === 'Enter' ? 'open' : e.key === 'Escape' ? 'back' : null
}
