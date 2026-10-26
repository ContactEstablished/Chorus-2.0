import { describe, expect, it, vi } from 'vitest'
import type { ProviderSummary, Result } from '../../ipc'
import type { CheckOutcome, ObservedModel, ObserverView } from '../../ipc/bindings'
import observationVue from './RoutingObservation.vue?raw'
import settingsVue from './RoutingSettings.vue?raw'
import {
  NO_CHECK_TEXT,
  NO_MODELS,
  OBSERVE_LABEL,
  OFF_LABEL,
  OFF_TEXT,
  STOP_LABEL,
  chooseProvider,
  modelRows,
  observerLines,
  observerProviders,
  outcomeText,
  overdue,
  setObserved,
} from './routingObservation'

const MIN = 60_000
const NOW = 1_700_000_000_000

function model(name: string, over: Partial<ObservedModel> = {}): ObservedModel {
  return { model: name, fetchedAtMs: NOW - 12 * MIN, sampleCount: 40, stopped: false, ...over }
}

function observer(over: Partial<ObserverView> = {}): ObserverView {
  return {
    providerId: 3,
    providerName: 'OpenRouter',
    problem: null,
    lastCheck: null,
    nextAtMs: NOW + 18 * MIN - 10_000,
    stoppedProblem: null,
    models: [],
    ...over,
  }
}

const checked = (outcome: CheckOutcome, atMs: number | null = NOW - 12 * MIN): ObserverView => observer({ lastCheck: { atMs, outcome } })
const texts = (v: ObserverView) => observerLines(v, NOW).map((l) => l.text)

describe('observerLines', () => {
  it('gives decisions.md’s Off text with no watched provider, and nothing else', () => {
    expect(OFF_TEXT).toBe("Off. Choose a provider to record endpoint numbers every 30 minutes with its key. It's free: no prompts or code are sent.")
    expect(observerLines(observer({ providerId: null, providerName: null }), NOW)).toEqual([{ text: OFF_TEXT, tone: 'normal' }])
  })

  it('stays off, with no last or next check, even when a check ran before it was turned off', () => {
    const v = observer({ providerId: null, providerName: null, lastCheck: { atMs: NOW - MIN, outcome: { kind: 'nothing-due' } } })
    expect(texts(v)).toEqual([OFF_TEXT])
  })

  it('says "On, with OpenRouter." for a watched provider named OpenRouter, then the check lines', () => {
    expect(texts(checked({ kind: 'updated', count: 3 }))).toEqual([
      'On, with OpenRouter.',
      'Last check 12 min ago: 3 models updated.',
      'Next check in 18 min.',
    ])
  })

  it('reads "No check since Chorus started." before any check, then the next check', () => {
    expect(NO_CHECK_TEXT).toBe('No check since Chorus started.')
    expect(texts(observer({ nextAtMs: NOW + 2 * MIN - 1 }))).toEqual(['On, with OpenRouter.', 'No check since Chorus started.', 'Next check in 2 min.'])
  })

  it('gives each outcome in decisions.md’s words', () => {
    const last = (outcome: CheckOutcome) => texts(checked(outcome))[1]
    expect(last({ kind: 'updated', count: 3 })).toBe('Last check 12 min ago: 3 models updated.')
    expect(last({ kind: 'updated', count: 1 })).toBe('Last check 12 min ago: 1 model updated.')
    expect(last({ kind: 'nothing-due' })).toBe('Last check 12 min ago: nothing was due.')
    expect(last({ kind: 'no-provider', reason: 'That provider no longer exists.' })).toBe(
      'Last check 12 min ago: no usable provider. That provider no longer exists.',
    )
    expect(last({ kind: 'key-refused' })).toBe('Last check 12 min ago: OpenRouter refused the key.')
    expect(last({ kind: 'rate-limited' })).toBe('Last check 12 min ago: OpenRouter rate-limited the check; the next check tries again.')
    expect(last({ kind: 'failed', message: 'Could not reach OpenRouter.' })).toBe('Last check 12 min ago: failed: Could not reach OpenRouter.')
  })

  it('tells the outcomes apart: no two read alike', () => {
    const outcomes: CheckOutcome[] = [
      { kind: 'updated', count: 2 },
      { kind: 'nothing-due' },
      { kind: 'no-provider', reason: 'r' },
      { kind: 'key-refused' },
      { kind: 'rate-limited' },
      { kind: 'failed', message: 'm' },
    ]
    expect(new Set(outcomes.map(outcomeText)).size).toBe(outcomes.length)
  })

  it('adds a full stop to a failure message only when it lacks one', () => {
    const failed = (message: string) => outcomeText({ kind: 'failed', message })
    expect(failed('Could not reach OpenRouter')).toBe('failed: Could not reach OpenRouter.')
    expect(failed('Could not reach OpenRouter.')).toBe('failed: Could not reach OpenRouter.')
    expect(failed('  Could not reach OpenRouter  ')).toBe('failed: Could not reach OpenRouter.')
    expect(failed('Is the key right?')).toBe('failed: Is the key right?')
    expect(failed('')).toBe('failed: no reason was given.')
  })

  it('shows the stopped-list failure the way the tick records it', () => {
    const message = "The list of stopped models couldn't be read. Stop or observe a model to replace it."
    expect(texts(checked({ kind: 'failed', message }))[1]).toBe(`Last check 12 min ago: failed: ${message}`)
  })

  it('counts a check that just ended as "just now"', () => {
    expect(texts(checked({ kind: 'nothing-due' }, NOW - 5_000))[1]).toBe('Last check just now: nothing was due.')
  })

  it('shows the provider’s problem, in the warning tone, after "On, with …"', () => {
    const problem = "Routing needs OpenRouter's own API key (a management key can't fetch endpoints)."
    const lines = observerLines(observer({ problem }), NOW)
    expect(lines.map((l) => l.text)).toEqual(['On, with OpenRouter.', problem, NO_CHECK_TEXT, 'Next check in 18 min.'])
    expect(lines.map((l) => l.tone)).toEqual(['normal', 'warn', 'normal', 'normal'])
  })

  it('shows no warning tone when the provider has no problem', () => {
    expect(observerLines(checked({ kind: 'updated', count: 3 }), NOW).every((l) => l.tone === 'normal')).toBe(true)
  })

  it('says so when the watched provider is gone, or a time is unknown, never zero', () => {
    const gone = observer({ providerName: null, problem: 'That provider no longer exists.', nextAtMs: null })
    expect(texts(gone)).toEqual(['On, with a provider that no longer exists.', 'That provider no longer exists.', NO_CHECK_TEXT, 'Next check unknown.'])
    expect(texts(checked({ kind: 'nothing-due' }, null))[1]).toBe('Last check at an unknown time: nothing was due.')
  })

  it('gives the next check in minutes rounded up, at least 1, then in hours', () => {
    expect(texts(observer({ nextAtMs: NOW + 17 * MIN + 10_000 }))[2]).toBe('Next check in 18 min.')
    expect(texts(observer({ nextAtMs: NOW - 5_000 }))[2]).toBe('Next check in 1 min.')
    expect(texts(observer({ nextAtMs: NOW + 130 * MIN }))[2]).toBe('Next check in 2 h.')
  })
})

describe('overdue', () => {
  it('is true once the next check has come, so the status is read again', () => {
    expect(overdue(observer({ nextAtMs: NOW + MIN }), NOW)).toBe(false)
    expect(overdue(observer({ nextAtMs: NOW }), NOW)).toBe(true)
    expect(overdue(observer({ nextAtMs: NOW - MIN }), NOW)).toBe(true)
  })

  it('is false with nothing loaded, or when the observer never started', () => {
    expect(overdue(null, NOW)).toBe(false)
    expect(overdue(observer({ nextAtMs: null }), NOW)).toBe(false)
  })
})

describe('modelRows', () => {
  it('gives "Updated 12 min ago · 40 samples" and offers Stop observing', () => {
    expect(modelRows(observer({ models: [model('a/x')] }), NOW)).toEqual([
      { model: 'a/x', status: 'Updated 12 min ago · 40 samples', stopped: false, mark: null, action: 'Stop observing', observe: false },
    ])
  })

  it('says "1 sample" for one', () => {
    expect(modelRows(observer({ models: [model('a/x', { sampleCount: 1 })] }), NOW)[0]?.status).toBe('Updated 12 min ago · 1 sample')
    expect(modelRows(observer({ models: [model('a/x', { sampleCount: 0 })] }), NOW)[0]?.status).toBe('Updated 12 min ago · 0 samples')
  })

  it('says "Not updated yet" with no snapshot time', () => {
    expect(modelRows(observer({ models: [model('a/x', { fetchedAtMs: null })] }), NOW)[0]?.status).toBe('Not updated yet')
  })

  it('marks a stopped model "Stopped" and offers Observe again, keeping what it shows of its data', () => {
    const [row] = modelRows(observer({ models: [model('a/x', { stopped: true })] }), NOW)
    expect(row).toEqual({ model: 'a/x', status: 'Updated 12 min ago · 40 samples', stopped: true, mark: 'Stopped', action: 'Observe again', observe: true })
    expect([STOP_LABEL, OBSERVE_LABEL]).toEqual(['Stop observing', 'Observe again'])
  })

  it('keeps the view’s order and lists nothing for no models', () => {
    const rows = modelRows(observer({ models: [model('a/x'), model('b/y', { stopped: true }), model('c/z')] }), NOW)
    expect(rows.map((r) => [r.model, r.action])).toEqual([
      ['a/x', 'Stop observing'],
      ['b/y', 'Observe again'],
      ['c/z', 'Stop observing'],
    ])
    expect(modelRows(observer(), NOW)).toEqual([])
    expect(NO_MODELS).toBe('No models yet. A model is followed once it has been refreshed.')
  })
})

function provider(id: number, name: string, over: Partial<ProviderSummary> = {}): ProviderSummary {
  return { id, kind: 'openrouter', name, credentialId: 1, ...over } as ProviderSummary
}

describe('observerProviders', () => {
  it('lists only OpenRouter providers', () => {
    const list = [provider(1, 'OpenRouter'), provider(2, 'Gateway', { kind: 'gateway' }), provider(3, 'Jev', { kind: 'jev' }), provider(4, 'Second')]
    expect(observerProviders(list).map((c) => c.id)).toEqual([1, 4])
  })

  it('leaves one with its own key enabled, with no title', () => {
    expect(observerProviders([provider(1, 'OpenRouter')])).toEqual([{ id: 1, label: 'OpenRouter', disabled: false, title: undefined }])
  })

  it('disables one without its own key, with the title that says why', () => {
    expect(observerProviders([provider(5, 'Work', { credentialId: null, managementCredentialId: 9 })])).toEqual([
      { id: 5, label: 'Work · API key needed', disabled: true, title: "Routing needs Work's own API key (a management key can't fetch endpoints)." },
    ])
  })

  it('gives no choices for no OpenRouter provider; the select still has Off', () => {
    expect(observerProviders([provider(2, 'Gateway', { kind: 'gateway' })])).toEqual([])
    expect(OFF_LABEL).toBe('Off')
  })
})

const ok = <T>(data: T): Result<T> => ({ status: 'ok', data })
const refused = (error: string): Result<never> => ({ status: 'error', error })

describe('chooseProvider', () => {
  it('saves the provider, then reads the status again', async () => {
    const after = observer({ providerId: 7, providerName: 'Work' })
    const watch = vi.fn(async () => ok(null))
    const load = vi.fn(async () => ok(after))
    expect(await chooseProvider(7, watch, load)).toEqual({ ok: true, view: after })
    expect(watch).toHaveBeenCalledWith(7)
    expect(load).toHaveBeenCalledTimes(1)
  })

  it('turns observation off with no provider', async () => {
    const off = observer({ providerId: null, providerName: null })
    const watch = vi.fn(async () => ok(null))
    expect(await chooseProvider(null, watch, async () => ok(off))).toEqual({ ok: true, view: off })
    expect(watch).toHaveBeenCalledWith(null)
  })

  it('returns Rust’s reason and reads nothing when the choice is refused', async () => {
    const load = vi.fn(async () => ok(observer()))
    const reason = "Routing needs Work's own API key (a management key can't fetch endpoints)."
    expect(await chooseProvider(5, async () => refused(reason), load)).toEqual({ ok: false, reason })
    expect(load).not.toHaveBeenCalled()
  })

  it('reports a failed reload, and a call that throws', async () => {
    expect(await chooseProvider(1, async () => ok(null), async () => refused('state is gone'))).toEqual({ ok: false, reason: 'state is gone' })
    const boom = async (): Promise<Result<null>> => {
      throw new Error('ipc closed')
    }
    expect(await chooseProvider(1, boom, async () => ok(observer()))).toEqual({ ok: false, reason: 'Error: ipc closed' })
  })
})

describe('setObserved', () => {
  it('asks for Stop observing with false and Observe again with true, and returns the new status', async () => {
    const after = observer({ models: [model('a/x', { stopped: true })] })
    const call = vi.fn(async () => ok(after))
    expect(await setObserved('a/x', false, call)).toEqual({ ok: true, view: after })
    expect(call).toHaveBeenLastCalledWith('a/x', false)
    await setObserved('a/x', true, call)
    expect(call).toHaveBeenLastCalledWith('a/x', true)
  })

  it('returns the refusal as Rust gives it, and a call that throws', async () => {
    const reason = "Chorus doesn't route Anthropic or OpenAI models through OpenRouter. Run them in Claude Code or Codex on your own sign-in."
    expect(await setObserved('openai/gpt-5.5', false, async () => refused(reason))).toEqual({ ok: false, reason })
    const boom = async (): Promise<Result<ObserverView>> => {
      throw new Error('ipc closed')
    }
    expect(await setObserved('a/x', false, boom)).toEqual({ ok: false, reason: 'Error: ipc closed' })
  })

  it('a stop followed by an observe again hands the model back to the observer', async () => {
    // What Rust does to the list, as `routing_observe_model` does it: stopped slugs, never data.
    let stopped: string[] = []
    const call = async (m: string, observe: boolean): Promise<Result<ObserverView>> => {
      stopped = observe ? stopped.filter((s) => s !== m) : [...stopped.filter((s) => s !== m), m]
      return ok(observer({ models: [model('a/x', { stopped: stopped.includes('a/x') })] }))
    }
    const first = await setObserved('a/x', false, call)
    expect(first.ok && modelRows(first.view, NOW)[0]?.action).toBe('Observe again')
    const second = await setObserved('a/x', true, call)
    expect(second.ok && modelRows(second.view, NOW)[0]?.action).toBe('Stop observing')
  })
})

describe('the section renders it, and the logic stays in the module', () => {
  it('RoutingObservation.vue takes its text and rows from routingObservation.ts and defines none of them', () => {
    expect(observationVue).toMatch(/from '\.\/routingObservation'/)
    for (const name of ['observerLines', 'modelRows', 'observerProviders', 'chooseProvider', 'setObserved', 'overdue']) {
      expect(observationVue).toContain(name)
      expect(observationVue).not.toMatch(new RegExp(`function ${name}\\(`))
    }
    expect(observationVue).not.toContain('Choose a provider to record')
    expect(observationVue).not.toContain('No check since Chorus started')
    expect(observationVue).toContain('{{ l.text }}')
    expect(observationVue).toContain('{{ r.status }}')
    expect(observationVue).toContain('{{ r.action }}')
  })

  it('uses the existing commands: routing_watch is the consent, and Stop observing keeps the data', () => {
    for (const name of ['commands.routingWatch', 'commands.routingObserver', 'commands.routingObserveModel']) expect(observationVue).toContain(name)
    expect(observationVue).not.toContain('commands.routingRefresh')
    expect(observationVue).not.toContain('commands.routingSettingsReset')
  })

  it('shows the warning tone, the stopped-list problem and the unknown states', () => {
    expect(observationVue).toContain(":class=\"{ warn: l.tone === 'warn' }\"")
    expect(observationVue).toContain('{{ view.stoppedProblem }}')
    expect(observationVue).toContain('{{ NO_MODELS }}')
    expect(observationVue).toContain('{{ OFF_LABEL }}')
  })

  it('every disabled control has a title, and a refused choice puts the select back', () => {
    expect(observationVue).toMatch(/<select v-model="choice" :disabled="!view \|\| busy" :title="selectTitle" @change="pick">/)
    expect(observationVue).toMatch(/:disabled="busy" :title="busy \? 'Working…' : undefined"/)
    expect(observationVue).toContain(':disabled="c.disabled" :title="c.title"')
    expect(observationVue).toMatch(/actionError\.value = r\.reason\s*choice\.value = view\.value\?\.providerId \?\? null/)
  })

  it('reloads on a new version and when the next check is due, and stops its clock when it goes', () => {
    expect(observationVue).toMatch(/defineProps<\{ version: number \}>/)
    expect(observationVue).toMatch(/watch\(\s*\(\) => props\.version/)
    expect(observationVue).toContain('!overdue(view.value, now.value)')
    expect(observationVue).toMatch(/onUnmounted\(\(\) => clearInterval\(ticker\)\)/)
  })

  it('RoutingSettings.vue shows it after the ranking settings', () => {
    expect(settingsVue).toMatch(/import RoutingObservation from '\.\/RoutingObservation\.vue'/)
    expect(settingsVue).toContain('<RoutingObservation :version="inspectVersion" />')
    expect(settingsVue.indexOf('<RoutingRanking')).toBeLessThan(settingsVue.indexOf('<RoutingObservation'))
  })
})
