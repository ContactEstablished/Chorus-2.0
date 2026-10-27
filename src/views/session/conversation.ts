/**
 * Pure helpers for the Friendly conversation: how a recorded turn reads on screen. Nothing here
 * decides session state; the recorded turns are the input and labels are the output.
 */
import type { PlanStep, TurnEntry } from '../../ipc'

/** A prompt a harness or Chorus injected (a background-task notice, a system reminder), not the user's words. */
export type SystemNotice = { title: string; detail: string | null }

function tag(xml: string, name: string): string | null {
  const m = new RegExp(`<${name}>([\\s\\S]*?)</${name}>`).exec(xml)
  return m ? m[1]!.trim() : null
}

/** Recognises machine-made prompts so the conversation can show them as a quiet system line. */
export function systemNotice(text: string | null): SystemNotice | null {
  if (!text) return null
  const trimmed = text.trim()
  if (trimmed.startsWith('<task-notification>')) {
    const status = tag(trimmed, 'status')
    const summary = tag(trimmed, 'summary')
    const title = `Background task ${status ?? 'update'}${summary ? `: ${summary}` : ''}`
    return { title, detail: trimmed }
  }
  if (trimmed.startsWith('<system-reminder>')) {
    const body = tag(trimmed, 'system-reminder') ?? ''
    const first = body.split('\n').find((l) => l.trim()) ?? 'System reminder'
    return { title: `System reminder: ${first.trim()}`, detail: trimmed }
  }
  return null
}

/** "45s", "3m 12s", "1h 02m"; null while the turn is still open. */
export function duration(startSecs: number, endSecs: number | null): string | null {
  if (endSecs === null || endSecs < startSecs) return null
  const total = Math.round(endSecs - startSecs)
  if (total < 60) return `${total}s`
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  if (h) return `${h}h ${String(m).padStart(2, '0')}m`
  return `${m}m ${String(s).padStart(2, '0')}s`
}

/** Today shows the time only; another day shows a short date as well. */
export function timeLabel(secs: number, now: Date = new Date()): string {
  const d = new Date(secs * 1000)
  const today = d.toDateString() === now.toDateString()
  return today
    ? d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
    : d.toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
}

export const LONG_PROMPT_LINES = 10
export const LONG_PROMPT_CHARS = 900

/** A prompt long enough to fold by default so the exchange around it stays scannable. */
export function isLongPrompt(text: string | null): boolean {
  if (!text) return false
  return text.length > LONG_PROMPT_CHARS || text.split('\n').length > LONG_PROMPT_LINES
}

/** How a turn ended, in a word, with the status class that colours it. */
export function endLabel(turn: Pick<TurnEntry, 'endReason'>): { label: string; cls: string } {
  switch (turn.endReason) {
    case null: return { label: 'In progress', cls: 'open' }
    case 'done': return { label: 'Done', cls: 'done' }
    case 'interrupted': return { label: 'Interrupted', cls: 'interrupted' }
    case 'failed': return { label: 'Failed', cls: 'failed' }
    case 'lost': return { label: 'Lost', cls: 'failed' }
    default: return { label: turn.endReason, cls: 'other' }
  }
}

export type Step = { id: number; kind: 'note' | 'proposed-plan'; text: string }

/** The agent's intermediate notes, in order, without a repeat of its final reply. */
export function steps(turn: Pick<TurnEntry, 'progress' | 'finalMessage'>): Step[] {
  const final = turn.finalMessage?.trim()
  const out: Step[] = []
  for (const p of turn.progress ?? []) {
    if (p.content.kind === 'message') {
      const text = p.content.text
      if (text.trim() && text.trim() !== final) out.push({ id: p.id, kind: 'note', text })
    } else if (p.content.kind === 'proposed-plan') {
      out.push({ id: p.id, kind: 'proposed-plan', text: p.content.text })
    }
  }
  return out
}

/** The newest recorded plan with at least one step, or null. */
export function latestPlan(turn: Pick<TurnEntry, 'progress'>): PlanStep[] | null {
  for (let i = (turn.progress ?? []).length - 1; i >= 0; i--) {
    const c = turn.progress[i]!.content
    if (c.kind === 'plan' && c.steps.length) return c.steps
  }
  return null
}

function count(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`
}

/** "4 notes · 7 tool calls · 1 approval", for the folded work group's summary line. */
export function workSummary(turn: Pick<TurnEntry, 'progress' | 'finalMessage' | 'tools' | 'asks'>): string {
  const parts: string[] = []
  const notes = steps(turn).length
  if (notes) parts.push(count(notes, 'note'))
  if (latestPlan(turn)) parts.push('a plan')
  if (turn.tools.length) parts.push(count(turn.tools.length, 'tool call'))
  const approvals = turn.asks.filter((a) => a.kind === 'approval').length
  const questions = turn.asks.length - approvals
  if (approvals) parts.push(count(approvals, 'approval'))
  if (questions) parts.push(count(questions, 'question'))
  return parts.join(' · ')
}

/** True when a turn has anything between its prompt and its reply worth folding. */
export function hasWork(turn: Pick<TurnEntry, 'progress' | 'finalMessage' | 'tools' | 'asks'>): boolean {
  return steps(turn).length > 0 || latestPlan(turn) !== null || turn.tools.length > 0 || turn.asks.length > 0
}

/**
 * How much has been recorded, as one number, so the view can say "3 new" when content arrives
 * while the reader is scrolled up: every prompt, note and reply counts once.
 */
export function eventCount(turns: readonly Pick<TurnEntry, 'progress' | 'finalMessage'>[]): number {
  let n = 0
  for (const t of turns) n += 1 + (t.progress?.length ?? 0) + (t.finalMessage ? 1 : 0)
  return n
}

/** A short source badge for prompts the user didn't type into this composer. */
export function sourceBadge(source: string | null): string | null {
  switch (source) {
    case null:
    case 'composer':
    case 'hook':
    case 'server':
      return null
    case 'telegram': return 'from your phone'
    case 'automation': return 'from an automation'
    case 'loop': return 'from the loop'
    default: return `from ${source}`
  }
}
