import { describe, expect, it } from 'vitest'
import type { CandidateExplanation, TiersView } from '../../ipc'
import vue from './RoutingTiers.vue?raw'
import { ago, details, inTime, pct, price, secs, tiersBlocked, tps, usd, why } from './routingFormat'

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

// Three eligible candidates (one below the Budget floor of 30) and one excluded.
const result = {
  budgetFloorTps: 30,
  candidates: [
    cand('a'),
    cand('b'),
    cand('c', { budgetFloorExcluded: true }),
    cand('d', { excludedBy: ['uptime below 99.5%'] }),
  ],
} as unknown as TierResult

describe('routing formatting, moved out of RoutingTiers.vue', () => {
  it('formats speed, price, uptime and latency, and shows unknown as a dash', () => {
    expect(tps(41.6)).toBe('42 tok/s')
    expect(price(0.1234)).toBe('$0.123/M')
    expect(price(2)).toBe('$2.00/M')
    expect(pct(99.5)).toBe('99.50%')
    expect(secs(0.4213)).toBe('0.42 s')
    for (const f of [tps, price, pct, secs]) {
      expect(f(null)).toBe('—')
      expect(f(undefined)).toBe('—')
    }
  })

  it('keeps the rounding it had', () => {
    expect(tps(0)).toBe('0 tok/s')
    expect(tps(41.4)).toBe('41 tok/s')
    expect(price(0.5)).toBe('$0.500/M')
    expect(price(1)).toBe('$1.00/M')
    expect(price(12.345)).toBe('$12.35/M')
    expect(pct(100)).toBe('100.00%')
    expect(secs(0)).toBe('0.00 s')
  })

  it('says how each tier chose, in Chorus 1’s words', () => {
    expect(why('budget', result)).toBe('First of 2 endpoints at or above the 30.0 tok/s Budget floor, scored mostly on price.')
    expect(why('balanced', result)).toBe('First of 3 eligible endpoints, scored equally on price and speed.')
    expect(why('fast', result)).toBe('First of 3 eligible endpoints, scored on speed only.')
  })

  it('says why with no result, as it did', () => {
    expect(why('budget', null)).toBe('First of 0 endpoints at or above the ? tok/s Budget floor, scored mostly on price.')
    expect(why('fast', null)).toBe('First of 0 eligible endpoints, scored on speed only.')
  })

  it('gives a candidate’s four detail lines', () => {
    const c = cand('deepinfra/fp8', {
      providerName: 'DeepInfra',
      cost: { blended: 0.1234, cacheCredit: true } as CandidateExplanation['cost'],
      tpsP50: 41.6,
      effectiveTps: 38.456,
      latencyP50S: 0.4213,
      uptime1d: 99.5,
      observations: 6,
    })
    expect(details(c)).toEqual([
      'deepinfra/fp8 (DeepInfra), fp8',
      '$0.123/M blended · 42 tok/s · 38.5 effective tok/s',
      '0.42 s latency · 99.50% uptime',
      'cache verified · 6 observations',
    ])
  })

  it('gives the detail lines of a candidate with nothing known', () => {
    expect(details(cand('x', { providerName: 'X', limitedHistory: true }))).toEqual([
      'x (X), fp8',
      '— blended · — · —',
      '— latency · — uptime',
      'cache not verified · limited history',
    ])
    expect(details(undefined)).toEqual([])
  })

  it('has no formatting left in RoutingTiers.vue', () => {
    const imported = /import \{([^}]*)\} from '\.\/routingFormat'/.exec(vue)?.[1]?.split(',').map((s) => s.trim()) ?? []
    // pct and secs are only used by details, which moved with them, so the component doesn't import them.
    for (const name of ['tps', 'price', 'why', 'details']) expect(imported).toContain(name)
    for (const name of ['tps', 'price', 'pct', 'secs', 'why', 'details']) {
      expect(vue).not.toMatch(new RegExp(`function ${name}\\(`))
      expect(vue).not.toMatch(new RegExp(`(const|let) ${name}\\b`))
    }
  })
})

describe('usd', () => {
  it('shows money under $1 with four decimals, otherwise two', () => {
    expect(usd(0.0123)).toBe('$0.0123')
    expect(usd(0)).toBe('$0.0000')
    expect(usd(0.05)).toBe('$0.0500')
    expect(usd(1.234)).toBe('$1.23')
    expect(usd(1)).toBe('$1.00')
  })

  it('shows unknown as a dash, not $0', () => {
    expect(usd(null)).toBe('—')
    expect(usd(undefined)).toBe('—')
    expect(usd(Number.NaN)).toBe('—')
  })
})

describe('ago', () => {
  const now = 1_000_000_000_000
  const MIN = 60_000

  it('says "just now" under a minute and for a time in the future', () => {
    expect(ago(now - 59_000, now)).toBe('just now')
    expect(ago(now, now)).toBe('just now')
    expect(ago(now + 5 * MIN, now)).toBe('just now')
  })

  it('counts minutes, hours and days, rounding down', () => {
    expect(ago(now - 60_000, now)).toBe('1 min ago')
    expect(ago(now - 12 * MIN - 40_000, now)).toBe('12 min ago')
    expect(ago(now - 59 * MIN, now)).toBe('59 min ago')
    expect(ago(now - 60 * MIN, now)).toBe('1 h ago')
    expect(ago(now - 3 * 60 * MIN - 20 * MIN, now)).toBe('3 h ago')
    expect(ago(now - 24 * 60 * MIN, now)).toBe('1 day ago')
    expect(ago(now - 47 * 60 * MIN, now)).toBe('1 day ago')
    expect(ago(now - 4 * 24 * 60 * MIN - 5 * 60 * MIN, now)).toBe('4 days ago')
  })
})

describe('inTime', () => {
  const now = 1_000_000_000_000
  const MIN = 60_000

  it('rounds minutes up, with at least one', () => {
    expect(inTime(now + 17 * MIN + 10_000, now)).toBe('in 18 min')
    expect(inTime(now + 18 * MIN, now)).toBe('in 18 min')
    expect(inTime(now + 5_000, now)).toBe('in 1 min')
    expect(inTime(now, now)).toBe('in 1 min')
    expect(inTime(now - 3 * MIN, now)).toBe('in 1 min')
    expect(inTime(now + 59 * MIN, now)).toBe('in 59 min')
  })

  it('counts hours from 60 minutes up', () => {
    expect(inTime(now + 60 * MIN, now)).toBe('in 1 h')
    expect(inTime(now + 2 * 60 * MIN + 10 * MIN, now)).toBe('in 2 h')
  })
})

describe('tiersBlocked', () => {
  const view = { model: 'deepseek/deepseek-v4.1-flash', result: null } as unknown as TiersView

  it('gives the error only when the tiers view failed to load', () => {
    expect(tiersBlocked(null, 'x')).toBe('x')
  })

  it('blocks nothing with a loaded view, or with no error', () => {
    expect(tiersBlocked(view, 'x')).toBeNull()
    expect(tiersBlocked(null, null)).toBeNull()
    expect(tiersBlocked(view, null)).toBeNull()
  })

  it('carries the refusal through as the reason', () => {
    const refusal = 'Chorus doesn’t route Anthropic or OpenAI models through OpenRouter.'
    expect(tiersBlocked(null, refusal)).toBe(refusal)
  })
})
