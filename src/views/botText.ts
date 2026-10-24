import type { Automation, AutomationRun, Bot } from '../ipc'

/** A Bot card's usage line (glacier-bots.png): what uses it now, else what it has done. */
export function botUsage(b: Pick<Bot, 'openSessions' | 'automations' | 'sessions'>): string {
  if (b.openSessions > 0) return `Used by ${b.openSessions} session${b.openSessions === 1 ? '' : 's'}`
  if (b.automations > 0) return `Triggered by ${b.automations === 1 ? 'an automation' : `${b.automations} automations`}`
  if (b.sessions > 0) return `Started ${b.sessions} session${b.sessions === 1 ? '' : 's'}`
  return 'Not used yet'
}

/** An automation row's status: a problem, paused, the next time, or how the last run went. `at`
 * formats a Unix-seconds time for display. */
export function automationStatus(a: Pick<Automation, 'problem' | 'enabled' | 'lastRun' | 'nextRunAtSecs'>, at: (secs: number | null) => string): { text: string; cls: string } {
  if (a.problem) return { text: 'can’t run', cls: 'bad' }
  if (!a.enabled) return { text: 'paused', cls: 'paused' }
  const r = a.lastRun
  if (!r) return { text: a.nextRunAtSecs ? `next ${at(a.nextRunAtSecs)}` : 'not run yet', cls: '' }
  if (r.outcome === 'started') return { text: `last run ✓ ${at(r.dueAtSecs)}`, cls: 'ok' }
  return { text: `last run ${r.outcome} ${at(r.dueAtSecs)}`, cls: r.outcome === 'failed' ? 'bad' : 'paused' }
}

/** One run in an automation's history, in words. */
export function runOutcome(r: Pick<AutomationRun, 'outcome' | 'sessionStatus' | 'note'>): string {
  if (r.outcome === 'started') return `Started a session${r.sessionStatus ? `, now ${r.sessionStatus}` : ''}`
  return r.note ?? r.outcome
}
