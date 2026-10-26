import { describe, expect, it } from 'vitest'
import type { CandidateExplanation, ProviderSummary, RefreshOutcome, TiersView } from '../../ipc'
import type { ObserverView } from '../../ipc/bindings'
import inspectVue from './RoutingInspect.vue?raw'
import settingsVue from './RoutingSettings.vue?raw'
import {
  DEFAULT_CARD,
  DEFAULT_LABEL,
  EFFORT_CHOICES,
  INTRO,
  NO_INPUT,
  REFRESH_TITLE,
  candidateHeading,
  candidateRows,
  defaultProviderId,
  effortLabel,
  inspectorProviders,
  modelSuggestions,
  nitroCard,
  refreshState,
  refreshText,
  registryLine,
  tierCards,
  updatedText,
} from './routingInspect'

type TierResult = NonNullable<TiersView['result']>

function cand(tag: string, over: Partial<CandidateExplanation> = {}): CandidateExplanation {
  return {
    tag,
    providerName: tag.toUpperCase(),
    quantization: 'fp8',
    excludedBy: [],
    budgetFloorExcluded: false,
    tpsP50: null,
    latencyP50S: null,
    effectiveTps: null,
    uptime1d: null,
    cost: null,
    limitedHistory: false,
    observations: 0,
    ...over,
  } as CandidateExplanation
}

const deepinfra = cand('deepinfra/fp8', {
  providerName: 'DeepInfra',
  tpsP50: 41.6,
  latencyP50S: 0.4213,
  uptime1d: 99.62,
  cost: { blended: 0.1234, cacheCredit: true } as CandidateExplanation['cost'],
})

function selection(endpoints: string[], rationale: string) {
  return { tier: 'balanced', model: 'deepseek/deepseek-v4.1-flash', endpoints, rationale }
}

function resultWith(over: Record<string, unknown> = {}): TierResult {
  return {
    model: 'deepseek/deepseek-v4.1-flash',
    stale: false,
    snapshotFetchedAtMs: null,
    budgetFloorTps: 30,
    candidates: [deepinfra, cand('b/fp8'), cand('c/fp8', { excludedBy: ['uptime below 99.5%', 'not fp8'] })],
    tiers: {
      budget: selection(['deepinfra/fp8', 'b/fp8', 'c/fp8'], 'Budget picked deepinfra/fp8.'),
      balanced: selection(['b/fp8'], 'Balanced picked b/fp8.'),
      fast: null,
    },
    nitro: { model: 'deepseek/deepseek-v4.1-flash:nitro', provider: null, likely: null, likelyFailsRules: [] },
    warnings: [],
    ...over,
  } as unknown as TierResult
}

function viewWith(over: Record<string, unknown> = {}, result: TierResult | null = resultWith()): TiersView {
  return {
    model: 'deepseek/deepseek-v4.1-flash',
    result,
    registry: null,
    remembered: null,
    preselect: 'default',
    preselectHint: null,
    watching: false,
    problem: null,
    retryInSecs: 0,
    ...over,
  } as unknown as TiersView
}

describe('the inspector’s fixed texts', () => {
  it('has decisions.md’s Routing intro', () => {
    expect(INTRO).toBe(
      "How OpenRouter's endpoints rank a model, for opencode's Budget, Balanced, Fast and Nitro tiers. Launches use a tier only when you choose one in New session.",
    )
  })

  it('has decisions.md’s OpenRouter default card', () => {
    expect(DEFAULT_LABEL).toBe('OpenRouter default')
    expect(DEFAULT_CARD).toBe("Chorus sends no routing: OpenRouter picks the provider for each request. The data-collection setting isn't sent.")
  })
})

describe('tierCards', () => {
  it('gives Budget, Balanced and Fast, in that order', () => {
    expect(tierCards(viewWith(), 60).map((c) => [c.tier, c.label])).toEqual([
      ['budget', 'Budget'],
      ['balanced', 'Balanced'],
      ['fast', 'Fast'],
    ])
  })

  it('shows the primary endpoint’s tag, provider, speed, price, latency and uptime', () => {
    const [budget] = tierCards(viewWith(), 60)
    expect(budget!.primary).toEqual({
      tag: 'deepinfra/fp8',
      provider: 'DeepInfra',
      speed: '42 tok/s',
      price: '$0.123/M',
      latency: '0.42 s',
      uptime: '99.62%',
    })
    expect(budget!.empty).toBeNull()
  })

  it('lists the fallbacks, or says there are none, then gives the rationale', () => {
    const [budget, balanced] = tierCards(viewWith(), 60)
    expect(budget!.fallbacks).toBe('Then: b/fp8, c/fp8')
    expect(budget!.rationale).toBe('Budget picked deepinfra/fp8.')
    expect(balanced!.fallbacks).toBe('No fallbacks')
    expect(balanced!.rationale).toBe('Balanced picked b/fp8.')
  })

  it('shows what it doesn’t know as a dash, not zero', () => {
    const [, balanced] = tierCards(viewWith(), 60)
    expect(balanced!.primary).toMatchObject({ tag: 'b/fp8', speed: '—', price: '—', latency: '—', uptime: '—' })
    const gone = viewWith({}, resultWith({ candidates: [] }))
    expect(tierCards(gone, 60)[0]!.primary).toEqual({ tag: 'deepinfra/fp8', provider: '—', speed: '—', price: '—', latency: '—', uptime: '—' })
  })

  it('asks for a Refresh when nothing is ranked yet', () => {
    for (const view of [viewWith({}, null), null]) {
      for (const card of tierCards(view, 60)) {
        expect(card.empty).toBe('Refresh to rank this model’s endpoints.')
        expect(card.primary).toBeNull()
        expect(card.fallbacks).toBeNull()
        expect(card.rationale).toBeNull()
        expect(card.stale).toBeNull()
      }
    }
  })

  it('says an empty tier has no eligible endpoints', () => {
    const fast = tierCards(viewWith(), 60)[2]!
    expect(fast.empty).toBe('Fast has no eligible endpoints.')
    expect(fast.primary).toBeNull()
    const noEndpoints = viewWith({}, resultWith({ tiers: { budget: selection([], 'x'), balanced: null, fast: null } }))
    expect(tierCards(noEndpoints, 60)[0]!.empty).toBe('Budget has no eligible endpoints.')
  })

  it('warns that a stale result needs a Refresh, with the saved limit', () => {
    const stale = viewWith({}, resultWith({ stale: true }))
    const [budget, balanced, fast] = tierCards(stale, 30)
    expect(budget!.stale).toBe('Older than 30 min: Refresh before launching with it.')
    expect(balanced!.stale).toBe('Older than 30 min: Refresh before launching with it.')
    // Fast has no endpoint, so there is nothing to launch with.
    expect(fast!.stale).toBeNull()
    expect(tierCards(viewWith(), 30).every((c) => c.stale === null)).toBe(true)
  })

  it('names no minutes when the saved limit couldn’t be read', () => {
    expect(tierCards(viewWith({}, resultWith({ stale: true })), null)[0]!.stale).toBe('Older than its limit: Refresh before launching with it.')
  })
})

describe('nitroCard', () => {
  const likely = { tag: 'fireworks/fp8', providerName: 'Fireworks', tpsP50: 120.4 }
  const billing = 'Requests may be billed at a provider’s priority-tier price. Your account’s guardrails still apply.'

  it('estimates the likely endpoint, then the billing and guardrails line', () => {
    const card = nitroCard(viewWith({}, resultWith({ nitro: { likely, likelyFailsRules: [] } })))
    expect(card.label).toBe('Nitro')
    expect(card.lines).toEqual(['Likely Fireworks (fireworks/fp8) at 120 tok/s: an estimate; actual routing may differ.', billing])
  })

  it('says which rules the likely endpoint fails', () => {
    const card = nitroCard(viewWith({}, resultWith({ nitro: { likely, likelyFailsRules: ['uptime below 99.5%', 'not fp8'] } })))
    expect(card.lines).toEqual([
      'Likely Fireworks (fireworks/fp8) at 120 tok/s: an estimate; actual routing may differ.',
      'That endpoint fails: uptime below 99.5%; not fp8.',
      billing,
    ])
  })

  it('says OpenRouter picks the fastest endpoint when there is no likely one', () => {
    expect(nitroCard(viewWith()).lines).toEqual(['OpenRouter picks the fastest endpoint for each request.', billing])
    expect(nitroCard(viewWith({}, null)).lines).toEqual(['OpenRouter picks the fastest endpoint for each request.', billing])
    expect(nitroCard(null).lines).toEqual(['OpenRouter picks the fastest endpoint for each request.', billing])
  })
})

describe('candidateRows', () => {
  it('lists every candidate with its facts, in the result’s order', () => {
    const rows = candidateRows(resultWith())
    expect(rows.map((r) => r.tag)).toEqual(['deepinfra/fp8', 'b/fp8', 'c/fp8'])
    expect(rows[0]).toEqual({
      tag: 'deepinfra/fp8',
      provider: 'DeepInfra',
      speed: '42 tok/s',
      price: '$0.123/M',
      latency: '0.42 s',
      uptime: '99.62%',
      reasons: '',
      excluded: false,
    })
  })

  it('joins the exclusion reasons with "; "', () => {
    const c = candidateRows(resultWith())[2]!
    expect(c.reasons).toBe('uptime below 99.5%; not fp8')
    expect(c.excluded).toBe(true)
  })

  it('says "below the Budget floor" for a candidate only the floor excludes', () => {
    const rows = candidateRows(resultWith({ candidates: [cand('slow/fp8', { budgetFloorExcluded: true })] }))
    expect(rows[0]).toMatchObject({ reasons: 'below the Budget floor', excluded: false })
  })

  it('keeps the rules when the floor excludes it too', () => {
    const rows = candidateRows(resultWith({ candidates: [cand('x', { excludedBy: ['not fp8'], budgetFloorExcluded: true })] }))
    expect(rows[0]!.reasons).toBe('not fp8')
  })

  it('has no rows without a result', () => {
    expect(candidateRows(null)).toEqual([])
    expect(candidateRows(undefined)).toEqual([])
  })

  it('counts eligible and excluded endpoints in its heading', () => {
    expect(candidateHeading(candidateRows(resultWith()))).toBe('Every endpoint (2 eligible · 1 excluded)')
    expect(candidateHeading([])).toBe('Every endpoint (0 eligible · 0 excluded)')
  })
})

describe('registryLine', () => {
  it('says what Chorus knows of the model, or that it was derived', () => {
    const context = (131_072).toLocaleString()
    const bundled = viewWith({ registry: { source: 'bundled', nativePrecision: 'fp8', minContext: 131_072 } })
    expect(registryLine(bundled)).toBe(`Chorus knows this model: at least fp8, ${context} tokens of context.`)
    const derived = viewWith({ registry: { source: 'derived', nativePrecision: null, minContext: 131_072 } })
    expect(registryLine(derived)).toBe(`Derived from its endpoints: at least any precision, ${context} tokens of context.`)
  })

  it('has no line when routing knows nothing of the model', () => {
    expect(registryLine(viewWith())).toBeNull()
    expect(registryLine(null)).toBeNull()
  })
})

describe('updatedText', () => {
  const now = 1_000_000_000_000

  it('says how long ago the endpoints were fetched', () => {
    expect(updatedText(viewWith({}, resultWith({ snapshotFetchedAtMs: now - 12 * 60_000 })), now)).toBe('Updated 12 min ago')
    expect(updatedText(viewWith({}, resultWith({ snapshotFetchedAtMs: now - 5_000 })), now)).toBe('Updated just now')
  })

  it('says nothing before the first Refresh', () => {
    expect(updatedText(viewWith(), now)).toBeNull()
    expect(updatedText(viewWith({}, null), now)).toBeNull()
    expect(updatedText(null, now)).toBeNull()
  })
})

describe('refreshState', () => {
  it('is disabled with the provider’s problem as its title', () => {
    const problem = 'Routing needs Work’s own API key (a management key can’t fetch endpoints).'
    expect(refreshState(viewWith({ problem }), false)).toEqual({ disabled: true, title: problem })
  })

  it('is disabled during the cooldown, saying when it ends', () => {
    expect(refreshState(viewWith({ retryInSecs: 42 }), false)).toEqual({
      disabled: true,
      title: 'Refreshed less than a minute ago: again in 42 s',
    })
  })

  it('is disabled while a Refresh runs', () => {
    expect(refreshState(viewWith(), true)).toEqual({ disabled: true, title: 'Refreshing…' })
  })

  it('is enabled otherwise, with the cost as its title', () => {
    const state = refreshState(viewWith(), false)
    expect(state).toEqual({ disabled: false, title: REFRESH_TITLE })
    expect(state.title).toContain('$0.05')
  })

  it('puts the provider’s problem before the cooldown, and both before busy', () => {
    expect(refreshState(viewWith({ problem: 'p', retryInSecs: 9 }), true).title).toBe('p')
    expect(refreshState(viewWith({ retryInSecs: 9 }), true).title).toBe('Refreshed less than a minute ago: again in 9 s')
  })

  it('is disabled, and says what to choose, with no view', () => {
    expect(refreshState(null, false)).toEqual({ disabled: true, title: NO_INPUT })
  })
})

describe('refreshText', () => {
  function outcome(over: Partial<RefreshOutcome>): RefreshOutcome {
    return { view: viewWith(), estimateUsd: 0.05, spentUsd: 0.0123, probed: [], notProbed: [], rejectedRows: 0, notes: [], ...over } as unknown as RefreshOutcome
  }

  it('says no cache test was needed when nothing was probed', () => {
    expect(refreshText(outcome({}))).toBe('Refreshed. No cache test needed: every eligible endpoint was tested in the last 14 days.')
  })

  it('gives the spend and the estimate through usd', () => {
    expect(refreshText(outcome({ probed: ['a', 'b'] }))).toBe('Refreshed. Tested prompt caching on 2 endpoints for $0.0123 (at most $0.0500).')
    expect(refreshText(outcome({ probed: ['a'], spentUsd: 0, estimateUsd: 1.234 }))).toBe(
      'Refreshed. Tested prompt caching on 1 endpoint for $0.0000 (at most $1.23).',
    )
  })

  it('shows an amount OpenRouter didn’t report as a dash, not $0', () => {
    expect(refreshText(outcome({ probed: ['a'], spentUsd: null }))).toBe('Refreshed. Tested prompt caching on 1 endpoint for — (at most $0.0500).')
  })
})

describe('provider choices', () => {
  const provider = (id: number, kind: ProviderSummary['kind'], name = `P${id}`) => ({ id, kind, name }) as ProviderSummary
  const providers = [provider(1, 'gateway'), provider(2, 'openrouter'), provider(3, 'ollama'), provider(4, 'openrouter'), provider(5, 'jev')]

  it('keeps only OpenRouter providers', () => {
    expect(inspectorProviders(providers).map((p) => p.id)).toEqual([2, 4])
    expect(inspectorProviders([])).toEqual([])
  })

  it('starts with the watched provider when it is an OpenRouter one', () => {
    expect(defaultProviderId(providers, 4)).toBe(4)
  })

  it('otherwise starts with the first OpenRouter provider', () => {
    expect(defaultProviderId(providers, null)).toBe(2)
    expect(defaultProviderId(providers, 1)).toBe(2) // watched, but not OpenRouter
    expect(defaultProviderId(providers, 99)).toBe(2) // watched, but gone
  })

  it('has none to start with when there is no OpenRouter provider', () => {
    expect(defaultProviderId([provider(1, 'gateway')], 1)).toBeNull()
    expect(defaultProviderId([], null)).toBeNull()
  })
})

describe('effort choices', () => {
  it('are no effort, then Chorus’s scale', () => {
    expect([...EFFORT_CHOICES]).toEqual([null, 'fast', 'balanced', 'deep', 'max'])
  })

  it('are labelled "No effort", then the labels the app uses elsewhere', () => {
    expect(EFFORT_CHOICES.map(effortLabel)).toEqual(['No effort', 'Fast', 'Balanced', 'Deep', 'Max'])
  })
})

describe('modelSuggestions', () => {
  const observer = (slugs: string[]) =>
    ({ models: slugs.map((model) => ({ model, fetchedAtMs: null, sampleCount: 0, stopped: false })) }) as unknown as ObserverView

  it('gives the observer’s model slugs in order', () => {
    expect(modelSuggestions(observer(['a/x', 'b/y', 'c/z']))).toEqual(['a/x', 'b/y', 'c/z'])
  })

  it('offers a stopped model too: its data is still stored', () => {
    const o = observer(['a/x'])
    o.models[0]!.stopped = true
    expect(modelSuggestions(o)).toEqual(['a/x'])
  })

  it('gives none before the observer view loads', () => {
    expect(modelSuggestions(null)).toEqual([])
    expect(modelSuggestions(observer([]))).toEqual([])
  })
})

describe('the section and the inspector render, and the logic stays in the module', () => {
  it('RoutingSettings.vue shows the intro and the inspector', () => {
    expect(settingsVue).toMatch(/import \{ INTRO \} from '\.\/routingInspect'/)
    expect(settingsVue).toContain('{{ INTRO }}')
    expect(settingsVue).toMatch(/import RoutingInspect from '\.\/RoutingInspect\.vue'/)
    expect(settingsVue).toContain('<RoutingInspect')
    expect(settingsVue).toContain('<h2>Routing</h2>')
  })

  it('RoutingInspect.vue takes its words and choices from routingInspect.ts', () => {
    expect(inspectVue).toMatch(/from '\.\/routingInspect'/)
    for (const name of ['tierCards', 'nitroCard', 'candidateRows', 'registryLine', 'refreshState', 'refreshText', 'inspectorProviders', 'defaultProviderId', 'modelSuggestions'])
      expect(inspectVue).toContain(name)
    expect(inspectVue).toContain('DEFAULT_CARD')
    for (const name of ['tierCards', 'nitroCard', 'candidateRows', 'refreshState', 'refreshText'])
      expect(inspectVue).not.toMatch(new RegExp(`function ${name}\\(`))
  })

  it('RoutingInspect.vue emits inspect and refreshed, and reloads on a new version', () => {
    expect(inspectVue).toMatch(/inspect: \[/)
    expect(inspectVue).toMatch(/refreshed: \[/)
    expect(inspectVue).toMatch(/version: number/)
    expect(inspectVue).toMatch(/watch\(\s*\(\) => props\.version/)
  })

  it('RoutingInspect.vue never chooses a tier for a launch', () => {
    expect(inspectVue).not.toContain('update:modelValue')
    expect(inspectVue).not.toContain('RoutingLaunchChoice')
    expect(inspectVue).not.toContain('routingWatch')
    expect(inspectVue).not.toContain('role="radio"')
  })
})
