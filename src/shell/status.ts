import type { AgentKind, Attention, GroupBy, Project, SessionSummary } from '../ipc'

/** DESIGN.md "Status vocabulary": the label and the card's call to action for each word. */
export const ATTENTION: Record<Attention, { label: string; cta: string }> = {
  approval: { label: 'Awaiting approval', cta: 'Review request' },
  input: { label: 'Needs your answer', cta: 'Answer' },
  failed: { label: 'Failed', cta: 'Recover' },
  working: { label: 'Working', cta: 'Watch' },
  ready: { label: 'Ready for next step', cta: 'Choose next step' },
  running: { label: 'Running', cta: 'Open' },
  done: { label: 'Completed', cta: 'Open' },
}

/** Most urgent first (matches the back end's order). */
export const URGENCY: Attention[] = ['approval', 'input', 'failed', 'working', 'ready', 'running', 'done']

export const AGENT_CHIPS: Record<AgentKind, string> = { claude: 'CC', codex: 'CX', opencode: 'OR', grok: 'GK', gemini: 'GM', shell: 'SH' }

export type StatusFilter = 'all' | 'needs' | 'working' | 'ready' | 'failed'

export function needsYou(a: Attention): boolean {
  return a === 'approval' || a === 'input' || a === 'failed'
}

export function matchesFilter(s: SessionSummary, f: StatusFilter): boolean {
  switch (f) {
    case 'all':
      return true
    case 'needs':
      return needsYou(s.attention)
    default:
      return s.attention === f
  }
}

export interface Counts {
  total: number
  needs: number
  working: number
  ready: number
  failed: number
}

export function counts(list: SessionSummary[]): Counts {
  const c: Counts = { total: list.length, needs: 0, working: 0, ready: 0, failed: 0 }
  for (const s of list) {
    if (needsYou(s.attention)) c.needs++
    if (s.attention === 'working') c.working++
    if (s.attention === 'ready') c.ready++
    if (s.attention === 'failed') c.failed++
  }
  return c
}

/** "3 sessions need you · 1 working · 6 total". */
export function headline(c: Counts): string {
  const parts = [c.needs === 1 ? '1 session needs you' : `${c.needs} sessions need you`]
  if (c.working) parts.push(`${c.working} working`)
  parts.push(`${c.total} total`)
  return parts.join(' · ')
}

/** Most urgent first, then the most recently active. */
export function byUrgency(a: SessionSummary, b: SessionSummary): number {
  return URGENCY.indexOf(a.attention) - URGENCY.indexOf(b.attention) || lastActive(b) - lastActive(a) || a.id - b.id
}

export function lastActive(s: SessionSummary): number {
  return Math.max(s.latestAtSecs, s.runStartedAtSecs ?? 0, s.createdAtSecs)
}

export interface Group {
  key: string
  title: string
  hint: string
  /** A project's colour, when grouped by project. */
  colour: string | null
  sessions: SessionSummary[]
}

/** The Overview's sections for a grouping. Empty status groups are left out. */
export function groupSessions(list: SessionSummary[], by: GroupBy, projects: Project[]): Group[] {
  const sorted = [...list].sort(byUrgency)
  if (by === 'none') return [{ key: 'all', title: 'All sessions', hint: 'most urgent first', colour: null, sessions: sorted }]
  if (by === 'status') {
    const groups: Group[] = [
      { key: 'needs', title: 'Needs you', hint: 'approvals, questions and failures — most urgent first', colour: null, sessions: [] },
      { key: 'flight', title: 'In flight', hint: 'working or ready for the next step', colour: null, sessions: [] },
      { key: 'finished', title: 'Finished', hint: 'completed', colour: null, sessions: [] },
    ]
    for (const s of sorted) {
      const i = needsYou(s.attention) ? 0 : s.attention === 'done' ? 2 : 1
      groups[i]!.sessions.push(s)
    }
    return groups.filter((g) => g.sessions.length)
  }
  const groups = projects.map<Group>((p) => ({ key: `p${p.id}`, title: p.name, hint: p.folder ?? '', colour: p.colour, sessions: [] }))
  const none: Group = { key: 'none', title: 'No project', hint: 'sessions outside every project folder', colour: null, sessions: [] }
  const index = new Map(projects.map((p, i) => [p.id, i]))
  for (const s of sorted) {
    const i = s.projectId === null ? undefined : index.get(s.projectId)
    ;(i === undefined ? none : groups[i]!).sessions.push(s)
  }
  return [...groups, none].filter((g) => g.sessions.length)
}

/** "2h 05m", "23m", "12s": how long ago, or how long, in the cards' short form. */
export function shortDuration(secs: number): string {
  const s = Math.max(0, Math.floor(secs))
  if (s < 60) return `${s}s`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h}h ${String(m % 60).padStart(2, '0')}m`
  return `${Math.floor(h / 24)}d ${h % 24}h`
}

export function ago(secs: number, now: number): string {
  if (!secs) return 'never'
  const d = now - secs
  return d < 10 ? 'just now' : `${shortDuration(d)} ago`
}

/** A folder for display: the user's home as `~`, forward slashes. */
export function shortPath(path: string, home: string | null): string {
  const p = path.replace(/\\/g, '/')
  const h = home?.replace(/\\/g, '/').replace(/\/$/, '')
  return h && p.toLowerCase().startsWith(h.toLowerCase()) ? `~${p.slice(h.length)}` : p
}
