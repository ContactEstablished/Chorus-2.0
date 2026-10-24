import { describe, expect, it } from 'vitest'
import type { AutomationRun } from '../ipc'
import { automationStatus, botUsage, runOutcome } from './botText'

const at = (secs: number | null) => (secs ? `t${secs}` : '')
const run = (over: Partial<AutomationRun>): AutomationRun =>
  ({ id: 1, automationId: 1, sessionId: null, trigger: 'schedule', dueAtSecs: 60, startedAtSecs: null, outcome: 'started', note: null, sessionStatus: null, ...over }) as AutomationRun

describe('Bot and automation wording', () => {
  it('says what uses a Bot, most current first', () => {
    expect(botUsage({ openSessions: 1, automations: 2, sessions: 5 })).toBe('Used by 1 session')
    expect(botUsage({ openSessions: 3, automations: 0, sessions: 3 })).toBe('Used by 3 sessions')
    expect(botUsage({ openSessions: 0, automations: 1, sessions: 4 })).toBe('Triggered by an automation')
    expect(botUsage({ openSessions: 0, automations: 2, sessions: 0 })).toBe('Triggered by 2 automations')
    expect(botUsage({ openSessions: 0, automations: 0, sessions: 1 })).toBe('Started 1 session')
    expect(botUsage({ openSessions: 0, automations: 0, sessions: 0 })).toBe('Not used yet')
  })

  it('puts a problem before paused, and the last run before the next time', () => {
    const base = { problem: null, enabled: true, lastRun: null, nextRunAtSecs: 120 }
    expect(automationStatus({ ...base, problem: 'Its Bot was deleted.' }, at)).toEqual({ text: 'can’t run', cls: 'bad' })
    expect(automationStatus({ ...base, enabled: false }, at)).toEqual({ text: 'paused', cls: 'paused' })
    expect(automationStatus(base, at)).toEqual({ text: 'next t120', cls: '' })
    expect(automationStatus({ ...base, nextRunAtSecs: null }, at).text).toBe('not run yet')
    expect(automationStatus({ ...base, lastRun: run({}) }, at)).toEqual({ text: 'last run ✓ t60', cls: 'ok' })
    expect(automationStatus({ ...base, lastRun: run({ outcome: 'failed' }) }, at).cls).toBe('bad')
    expect(automationStatus({ ...base, lastRun: run({ outcome: 'missed' }) }, at)).toEqual({ text: 'last run missed t60', cls: 'paused' })
  })

  it('describes each run with its reason', () => {
    expect(runOutcome(run({ sessionStatus: 'finished its turn' }))).toBe('Started a session, now finished its turn')
    expect(runOutcome(run({ outcome: 'skipped', note: 'Skipped: the previous run is still busy.' }))).toBe('Skipped: the previous run is still busy.')
    expect(runOutcome(run({ outcome: 'missed' }))).toBe('missed')
  })
})
