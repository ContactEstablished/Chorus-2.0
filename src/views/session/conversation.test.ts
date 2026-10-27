import { describe, expect, it } from 'vitest'
import type { TurnEntry } from '../../ipc'
import { duration, endLabel, eventCount, hasWork, isLongPrompt, latestPlan, sourceBadge, steps, systemNotice, timeLabel, workSummary } from './conversation'

const turn = (over: Partial<TurnEntry> = {}): TurnEntry => ({
  id: 1, prompt: 'hi', promptSource: 'hook', startedAtSecs: 1000, endedAtSecs: 1192, endReason: 'done',
  finalMessage: 'All done.', tools: [], asks: [], progress: [], ...over,
})

describe('systemNotice', () => {
  it('reads a background-task notification into a one-line title', () => {
    const text = `<task-notification>\n<task-id>abc</task-id>\n<status>completed</status>\n<summary>Background command "Run the gate" completed (exit code 0)</summary>\n</task-notification>`
    expect(systemNotice(text)).toEqual({ title: 'Background task completed: Background command "Run the gate" completed (exit code 0)', detail: text })
  })
  it('reads a system reminder by its first line', () => {
    expect(systemNotice('<system-reminder>\nThe file changed on disk.\nMore.\n</system-reminder>')?.title).toBe('System reminder: The file changed on disk.')
  })
  it('leaves ordinary prompts alone, even ones that mention the tags', () => {
    expect(systemNotice('Please read the <task-notification> format and explain it')).toBeNull()
    expect(systemNotice(null)).toBeNull()
    expect(systemNotice('')).toBeNull()
  })
})

describe('duration', () => {
  it('formats seconds, minutes and hours', () => {
    expect(duration(0, 45)).toBe('45s')
    expect(duration(0, 192)).toBe('3m 12s')
    expect(duration(0, 3725)).toBe('1h 02m')
  })
  it('is null while the turn is open or when the clock went backwards', () => {
    expect(duration(10, null)).toBeNull()
    expect(duration(10, 5)).toBeNull()
  })
})

describe('timeLabel', () => {
  it('shows only the time for today and adds the date otherwise', () => {
    const now = new Date(2026, 9, 26, 12, 0, 0)
    const today = new Date(2026, 9, 26, 10, 48).getTime() / 1000
    const yesterday = new Date(2026, 9, 25, 22, 48).getTime() / 1000
    expect(timeLabel(today, now)).not.toMatch(/Oct/)
    expect(timeLabel(yesterday, now)).toMatch(/Oct/)
  })
})

describe('isLongPrompt', () => {
  it('folds by lines or by characters', () => {
    expect(isLongPrompt('short')).toBe(false)
    expect(isLongPrompt(null)).toBe(false)
    expect(isLongPrompt(Array(12).fill('line').join('\n'))).toBe(true)
    expect(isLongPrompt('x'.repeat(901))).toBe(true)
  })
})

describe('steps and plans', () => {
  it('keeps notes in order and drops a repeat of the final reply', () => {
    const t = turn({ progress: [
      { id: 1, content: { kind: 'message', text: 'Reading.' } },
      { id: 2, content: { kind: 'plan', steps: [{ id: 'a', text: 'Read', status: 'completed' }] } },
      { id: 3, content: { kind: 'message', text: 'All done.' } },
      { id: 4, content: { kind: 'proposed-plan', text: '1. Do it' } },
      { id: 5, content: { kind: 'message', text: '   ' } },
    ] })
    expect(steps(t)).toEqual([{ id: 1, kind: 'note', text: 'Reading.' }, { id: 4, kind: 'proposed-plan', text: '1. Do it' }])
    expect(latestPlan(t)).toEqual([{ id: 'a', text: 'Read', status: 'completed' }])
  })
  it('summarises the folded work', () => {
    const t = turn({
      progress: [{ id: 1, content: { kind: 'message', text: 'Reading.' } }],
      tools: [{ tool: 'Read', target: 'a.rs', failed: false, finished: true }, { tool: 'Bash', target: null, failed: true, finished: true }],
      asks: [{ id: 1, kind: 'approval', subject: 'Bash', answer: 'allow', details: null, replyAttempted: true, replyAction: null }],
    })
    expect(workSummary(t)).toBe('1 note · 2 tool calls · 1 approval')
    expect(hasWork(t)).toBe(true)
    expect(hasWork(turn())).toBe(false)
    expect(workSummary(turn())).toBe('')
  })
})

describe('endLabel and badges', () => {
  it('names each ending', () => {
    expect(endLabel({ endReason: null })).toEqual({ label: 'In progress', cls: 'open' })
    expect(endLabel({ endReason: 'done' }).cls).toBe('done')
    expect(endLabel({ endReason: 'lost' }).cls).toBe('failed')
    expect(endLabel({ endReason: 'odd' })).toEqual({ label: 'odd', cls: 'other' })
  })
  it('badges only prompts that came from elsewhere', () => {
    expect(sourceBadge('composer')).toBeNull()
    expect(sourceBadge('hook')).toBeNull()
    expect(sourceBadge(null)).toBeNull()
    expect(sourceBadge('telegram')).toBe('from your phone')
    expect(sourceBadge('loop')).toBe('from the loop')
  })
})

describe('eventCount', () => {
  it('counts prompts, notes and replies once each', () => {
    expect(eventCount([])).toBe(0)
    expect(eventCount([turn({ progress: [{ id: 1, content: { kind: 'message', text: 'a' } }] }), turn({ finalMessage: null })])).toBe(4)
  })
})
