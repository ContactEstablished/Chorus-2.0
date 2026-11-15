import { describe, expect, it } from 'vitest'
import type { SessionProgress, TimeSplit, TimedStep, ToolGroup } from '../../ipc'
import { elapsedLabel, emptyLine, liveSecs, nowNext, skillChip, splitAria, splitLegend, splitNote, spokenDuration, stepDuration, stepTime, stopped, toolGroupLabel, toolSummary } from './progress'

const step = (over: Partial<TimedStep> = {}): TimedStep => ({
  kind: 'step', text: 'Build', status: 'completed', startedAtSecs: 100, secs: 300, runningSinceSecs: null, timed: true, skills: [], ...over,
})
const group = (over: Partial<ToolGroup> = {}): ToolGroup => ({
  kind: 'read', tool: 'Read', count: 1, target: null, secs: 1, runningSinceSecs: null, failed: 0, ...over,
})

describe('stepDuration', () => {
  it('rounds to the units the panel shows', () => {
    expect(stepDuration(0)).toBe('<1 sec')
    expect(stepDuration(59)).toBe('59 sec')
    expect(stepDuration(60)).toBe('1 min')
    expect(stepDuration(89)).toBe('1 min')
    expect(stepDuration(59.4 * 60)).toBe('59 min')
    expect(stepDuration(59.5 * 60)).toBe('1 h')
    expect(stepDuration(1.5 * 3600)).toBe('1.5 h')
    expect(stepDuration(10 * 3600)).toBe('10 h')
    expect(stepDuration(12.4 * 3600)).toBe('12 h')
  })
})

describe('elapsedLabel', () => {
  it('reads like the mockup header', () => {
    expect(elapsedLabel(40)).toBe('40 sec')
    expect(elapsedLabel(47 * 60 + 5)).toBe('47 min')
    expect(elapsedLabel(64 * 60)).toBe('1 h 04 min')
  })
})

describe('liveSecs and stepTime', () => {
  it('adds the open interval only while running', () => {
    expect(liveSecs({ secs: 30, runningSinceSecs: 1000 }, 1090)).toBe(120)
    expect(liveSecs({ secs: 30, runningSinceSecs: null }, 1090)).toBe(30)
    expect(liveSecs({ secs: 0, runningSinceSecs: 2000 }, 1000)).toBe(0)
  })
  it('says not timed, marks skill rows as estimates and leaves pending steps blank', () => {
    expect(stepTime(step({ timed: false, secs: 0 }), 0)).toBe('not timed')
    expect(stepTime(step({ kind: 'skill', secs: 720 }), 0)).toBe('~12 min')
    expect(stepTime(step({ status: 'pending', startedAtSecs: null, secs: 0 }), 0)).toBe('')
    expect(stepTime(step({ status: 'in-progress', secs: 0, runningSinceSecs: 1000 }), 1300)).toBe('5 min')
  })
  it('knows a step whose run ended', () => {
    expect(stopped(step({ status: 'in-progress' }))).toBe(true)
    expect(stopped(step({ status: 'in-progress', runningSinceSecs: 1 }))).toBe(false)
  })
})

describe('tool groups', () => {
  it('labels each kind', () => {
    expect(toolGroupLabel(group({ count: 4 }))).toBe('Read 4 files')
    expect(toolGroupLabel(group({ kind: 'edit', tool: 'Edit', count: 2 }))).toBe('Edited 2 files')
    expect(toolGroupLabel(group({ kind: 'run', tool: 'Bash', target: 'cargo test' }))).toBe('Ran cargo test')
    expect(toolGroupLabel(group({ kind: 'run', tool: 'Bash', count: 3 }))).toBe('Ran 3 commands')
    expect(toolGroupLabel(group({ kind: 'search', tool: 'Grep', count: 5 }))).toBe('Searched 5 times')
    expect(toolGroupLabel(group({ kind: 'web', tool: 'WebFetch', count: 2 }))).toBe('Fetched 2 pages')
    expect(toolGroupLabel(group({ kind: 'agent', tool: 'Agent' }))).toBe('Ran 1 subagent')
    expect(toolGroupLabel(group({ kind: 'other', tool: 'PowerShell', count: 3 }))).toBe('PowerShell ×3')
  })
  it('joins them lower-case after the first', () => {
    expect(toolSummary([group({ count: 4 }), group({ kind: 'run', tool: 'Bash', target: 'cargo test' })])).toBe('Read 4 files · ran cargo test')
    expect(toolSummary([])).toBe('')
  })
})

describe('skill chips', () => {
  it('always carry the estimate mark', () => {
    expect(skillChip('grilling', 300)).toBe('grilling ~5 min')
    expect(skillChip(null, 30)).toBe('skill ~30 sec')
  })
})

describe('nowNext and emptyLine', () => {
  const p = (over: Partial<SessionProgress> = {}): SessionProgress => ({
    steps: [step({ text: 'A' }), step({ text: 'B', status: 'in-progress', secs: 0, runningSinceSecs: 100 }), step({ text: 'C', status: 'pending', startedAtSecs: null, secs: 0 })],
    current: 1, next: 2, done: 1, total: 3, currentTools: [], planAtSecs: 100, capped: false, split: null, ...over,
  })
  it('names the current and next steps', () => {
    expect(nowNext(p(), 400)).toEqual({ now: 'B', nowSecs: '5 min', next: 'C', done: false })
    expect(nowNext(p({ current: null, next: null, done: 3 }), 0)).toMatchObject({ now: null, next: null, done: true })
  })
  it('is honest about opencode', () => {
    expect(emptyLine('opencode')).toContain("doesn't read opencode's checklist")
    expect(emptyLine('claude')).toContain('No plan yet')
  })
})

describe('the time split', () => {
  const split = (over: Partial<TimeSplit> = {}): TimeSplit => ({
    notSplit: null, workingSecs: 47 * 60, youSecs: 13 * 60, approvalSecs: 4 * 60, idleSecs: 20 * 60,
    startedAtSecs: 1000, endedAtSecs: null, turns: 7, ...over,
  })
  it('lists four parts in order with rounded figures', () => {
    const odd = split({ workingSecs: 47 * 60 + 29, youSecs: 12 * 60 + 31, approvalSecs: 4 * 60 - 20, idleSecs: 20 * 60 + 10 })
    expect(splitLegend(odd).map((p) => [p.key, p.label, stepDuration(p.secs)])).toEqual([
      ['working', 'Agent working', '47 min'],
      ['you', 'Waiting on you', '13 min'],
      ['approval', 'Waiting on approval', '4 min'],
      ['idle', 'Idle', '20 min'],
    ])
  })
  it('drops empty parts', () => {
    expect(splitLegend(split({ approvalSecs: 0, idleSecs: 0 })).map((p) => p.key)).toEqual(['working', 'you'])
  })
  it('reads the bar aloud', () => {
    expect(splitAria(split())).toBe('Agent working 47 minutes, waiting on you 13 minutes, waiting on approval 4 minutes, idle 20 minutes')
    expect(splitAria(split({ workingSecs: 0, youSecs: 1, approvalSecs: 0, idleSecs: 5400 }))).toBe('Waiting on you 1 second, idle 1.5 hours')
    expect(spokenDuration(0)).toBe('under a second')
    expect(spokenDuration(60)).toBe('1 minute')
  })
  it('says why when not split, and has no bar', () => {
    const old = split({ notSplit: 'Recorded before Chorus kept state changes.' })
    expect(splitLegend(old)).toEqual([])
    expect(splitNote(old)).toBe('Where the time went: not split. Recorded before Chorus kept state changes.')
    expect(splitNote(split())).toBeNull()
    const silent = split({ notSplit: 'No events from the agent yet.', youSecs: 0 })
    expect(splitNote(silent)).toBe('Where the time went: not split. No events from the agent yet.')
  })
  it('says so when every part rounds to nothing', () => {
    expect(splitNote(split({ workingSecs: 0, youSecs: 0, approvalSecs: 0, idleSecs: 0 }))).toBe('Where the time went: nothing recorded yet.')
  })
})
