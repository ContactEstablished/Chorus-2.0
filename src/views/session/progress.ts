/**
 * Wording for the Progress panel (P16). Every number comes from `chorus_core::step_times`; this only
 * words it and adds the ticking part of a step that is still running (K5).
 */
import type { AgentKind, ProgressGlance, SessionProgress, StepStatus, TimeSplit, TimedStep, ToolGroup } from '../../ipc'

/** A step's rounded duration: "40 sec", "5 min", "1.5 h", "12 h". */
export function stepDuration(secs: number): string {
  const s = Math.max(0, Math.round(secs))
  if (s === 0) return '<1 sec'
  if (s < 60) return `${s} sec`
  if (s < 59.5 * 60) return `${Math.max(1, Math.round(s / 60))} min`
  const h = s / 3600
  if (h < 10) {
    const one = Math.round(h * 10) / 10
    return `${Number.isInteger(one) ? one.toFixed(0) : one.toFixed(1)} h`
  }
  return `${Math.round(h)} h`
}

/** The panel header's total: "1 h 04 min", "47 min", "40 sec". */
export function elapsedLabel(secs: number): string {
  const s = Math.max(0, Math.floor(secs))
  if (s < 60) return `${s} sec`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m} min`
  return `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, '0')} min`
}

/** Closed seconds plus the open interval, while it runs. */
export function liveSecs(t: { secs: number; runningSinceSecs: number | null }, now: number): number {
  return t.secs + (t.runningSinceSecs === null ? 0 : Math.max(0, now - t.runningSinceSecs))
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`

/** One group of tool calls, in words: "Read 4 files", "Ran cargo test", "PowerShell ×3". */
export function toolGroupLabel(g: ToolGroup): string {
  switch (g.kind) {
    case 'read': return `Read ${plural(g.count, 'file', 'files')}`
    case 'edit': return `Edited ${plural(g.count, 'file', 'files')}`
    case 'run': return g.count === 1 && g.target ? `Ran ${g.target}` : `Ran ${plural(g.count, 'command', 'commands')}`
    case 'search': return `Searched ${g.count === 1 ? 'once' : `${g.count} times`}`
    case 'web': return `Fetched ${plural(g.count, 'page', 'pages')}`
    case 'agent': return `Ran ${plural(g.count, 'subagent', 'subagents')}`
    default: return g.count === 1 ? g.tool : `${g.tool} ×${g.count}`
  }
}

/** The groups joined, lower-case after the first: "Read 4 files · ran cargo test". */
export function toolSummary(groups: ToolGroup[]): string {
  return groups.map((g, i) => {
    const label = toolGroupLabel(g)
    return i === 0 || g.kind === 'other' ? label : label.charAt(0).toLowerCase() + label.slice(1)
  }).join(' · ')
}

/** A skill chip's text: "grilling ~12 min". Its time is always an estimate (D115). */
export function skillChip(name: string | null, secs: number): string {
  return `${name ?? 'skill'} ~${stepDuration(secs)}`
}

/** A skill chip's tooltip. */
export function skillTitle(name: string | null, secs: number): string {
  if (name === null) return 'Name not recorded. Estimate: from the skill call until the next step started.'
  return `Skill ${name}, about ${stepDuration(secs)}. Estimate: from the skill call until the next step started.`
}

/** A step's duration text, or "not timed" when Chorus never saw it in progress (G3). */
export function stepTime(t: TimedStep, now: number): string {
  if (!t.timed) return 'not timed'
  if (t.status === 'pending' && t.startedAtSecs === null) return ''
  return t.kind === 'skill' ? `~${stepDuration(liveSecs(t, now))}` : stepDuration(liveSecs(t, now))
}

/** A step left in progress whose run ended: its time no longer grows. */
export function stopped(t: TimedStep): boolean {
  return t.status === 'in-progress' && t.runningSinceSecs === null
}

/** The collapsed line's parts. */
export function nowNext(p: SessionProgress, now: number): { now: string | null; nowSecs: string; next: string | null; done: boolean } {
  const cur = p.current === null ? null : p.steps[p.current] ?? null
  const next = p.next === null ? null : p.steps[p.next]?.text ?? null
  return {
    now: cur?.text ?? null,
    nowSecs: cur ? stepTime(cur, now) : '',
    next,
    done: p.total > 0 && p.done === p.total,
  }
}

/** What an empty panel says, honestly (K10). */
export function emptyLine(agent: AgentKind): string {
  return agent === 'opencode'
    ? "Chorus doesn't read opencode's checklist yet, so there is no plan here."
    : 'No plan yet. Steps appear when the agent writes a checklist or runs a skill.'
}

export type SplitKey = 'working' | 'you' | 'approval' | 'idle'
export type SplitPart = { key: SplitKey; label: string; secs: number }

/** "Where the time went", in legend order, with empty parts left out (16-2, K6). None when not split. */
export function splitLegend(s: TimeSplit): SplitPart[] {
  if (s.notSplit !== null) return []
  const all: SplitPart[] = [
    { key: 'working', label: 'Agent working', secs: s.workingSecs },
    { key: 'you', label: 'Waiting on you', secs: s.youSecs },
    { key: 'approval', label: 'Waiting on approval', secs: s.approvalSecs },
    { key: 'idle', label: 'Idle', secs: s.idleSecs },
  ]
  return all.filter((p) => p.secs > 0)
}

/** The one line shown instead of the bar, or null when there is a bar. */
export function splitNote(s: TimeSplit): string | null {
  if (s.notSplit !== null) return `Where the time went: not split. ${s.notSplit}`
  return splitLegend(s).length ? null : 'Where the time went: nothing recorded yet.'
}

/** A duration read aloud: "47 minutes", "1.5 hours", "under a second". */
export function spokenDuration(secs: number): string {
  const short = stepDuration(secs)
  if (short === '<1 sec') return 'under a second'
  const [n, unit] = short.split(' ')
  const word = unit === 'sec' ? 'second' : unit === 'min' ? 'minute' : 'hour'
  return `${n} ${n === '1' ? word : `${word}s`}`
}

/** The split bar's accessible label: "Agent working 47 minutes, waiting on you 13 minutes". */
export function splitAria(s: TimeSplit): string {
  return splitLegend(s).map((p, i) => `${i === 0 ? p.label : p.label.toLowerCase()} ${spokenDuration(p.secs)}`).join(', ')
}

/** The most steps an Overview card draws as separate segments; more becomes one continuous bar. */
export const CARD_SEGMENTS = 24

/** The card's line above its bar (16-3). Only a live run's current step has a ticking time. */
export function cardLine(g: ProgressGlance, live: boolean, now: number): { label: string; step: string; secs: number | null } {
  if (g.current !== null) {
    const since = live ? g.currentSinceSecs : null
    return { label: live ? 'Now:' : 'Stopped at:', step: g.current, secs: since === null ? null : Math.max(0, now - since) }
  }
  if (g.next !== null) return { label: 'Next:', step: g.next, secs: null }
  return { label: 'All steps done', step: '', secs: null }
}

export type CardBar =
  | { kind: 'segments'; statuses: StepStatus[] }
  | { kind: 'continuous'; donePct: number; currentPct: number }

/** One segment per step up to CARD_SEGMENTS, else one bar: done, then the steps in progress. */
export function cardSegments(g: ProgressGlance): CardBar {
  if (g.statuses.length <= CARD_SEGMENTS) return { kind: 'segments', statuses: g.statuses }
  const n = g.statuses.length
  const running = g.statuses.filter((x) => x === 'in-progress').length
  return { kind: 'continuous', donePct: (g.done / n) * 100, currentPct: (running / n) * 100 }
}
