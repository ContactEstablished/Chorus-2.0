/**
 * What Settings → Routing's inspector shows, as plain functions: the tier cards, the candidate
 * table, Refresh's state and text, and the provider, model and effort choices. The words are
 * RoutingTiers.vue's where it has them (Chorus 1's), so the inspector and New session agree.
 * Types only from the IPC layer, so tests run without Tauri.
 */
import type { Effort, ProviderSummary, RefreshOutcome, TiersView } from '../../ipc'
// ObserverView isn't re-exported by the IPC index, which no task of this phase claims.
import type { ObserverView } from '../../ipc/bindings'
import { EFFORT_LABELS } from '../../raw/labels'
import { ago, pct, price, secs, tps, usd } from '../newSession/routingFormat'

type TierResult = NonNullable<TiersView['result']>

export const INTRO =
  "How OpenRouter's endpoints rank a model, for opencode's Budget, Balanced, Fast and Nitro tiers. Launches use a tier only when you choose one in New session."

export const DEFAULT_LABEL = 'OpenRouter default'
export const DEFAULT_CARD = "Chorus sends no routing: OpenRouter picks the provider for each request. The data-collection setting isn't sent."

/** Shown while the inputs don't name a provider and a model, so there is nothing to rank. */
export const NO_INPUT = 'Choose an OpenRouter provider and enter a model to see how its endpoints rank.'

/** RoutingTiers' Refresh title and busy line. */
export const REFRESH_TITLE =
  'Fetches the endpoint list and checks what your account allows (both free), then may spend up to about $0.05 of OpenRouter credit testing prompt caching.'
export const REFRESHING = 'Fetching endpoints, checking the account, then testing prompt caching (at most $0.05)…'

/** What the inspector is looking at; emitted as `inspect` so the history follows it. */
export interface InspectInput {
  providerId: number | null
  model: string
  effort: Effort | null
}

export type Tier = 'budget' | 'balanced' | 'fast'
const TIERS: Tier[] = ['budget', 'balanced', 'fast']
const TIER_LABELS: Record<Tier, string> = { budget: 'Budget', balanced: 'Balanced', fast: 'Fast' }

/** One endpoint's facts, as shown. */
export interface EndpointFacts {
  tag: string
  provider: string
  speed: string
  price: string
  latency: string
  uptime: string
}

export interface TierCard {
  tier: Tier
  label: string
  /** The tier's first endpoint; null when there is none to show. */
  primary: EndpointFacts | null
  /** "Then: a, b" or "No fallbacks"; null without a primary. */
  fallbacks: string | null
  rationale: string | null
  /** Why there is no primary: nothing ranked yet, or no endpoint is eligible. */
  empty: string | null
  /** Set when the ranking is older than the saved limit. */
  stale: string | null
}

/**
 * The Budget, Balanced and Fast cards. `maxAgeMinutes` is the saved `snapshotMaxAgeMinutes`, which
 * decides when a ranking is stale; null when the saved settings couldn't be read.
 */
export function tierCards(view: TiersView | null, maxAgeMinutes: number | null): TierCard[] {
  const result = view?.result ?? null
  const byTag = new Map((result?.candidates ?? []).map((c) => [c.tag, c] as const))
  const limit = maxAgeMinutes === null ? 'its limit' : `${maxAgeMinutes} min`
  return TIERS.map((tier) => {
    const label = TIER_LABELS[tier]
    const none = { tier, label, primary: null, fallbacks: null, rationale: null, stale: null }
    if (!result) return { ...none, empty: 'Refresh to rank this model’s endpoints.' }
    const sel = result.tiers[tier]
    const tag = sel?.endpoints[0]
    if (!sel || tag === undefined) return { ...none, empty: `${label} has no eligible endpoints.` }
    const c = byTag.get(tag)
    return {
      tier,
      label,
      primary: {
        tag,
        provider: c?.providerName ?? '—',
        speed: tps(c?.tpsP50),
        price: price(c?.cost?.blended),
        latency: secs(c?.latencyP50S),
        uptime: pct(c?.uptime1d),
      },
      fallbacks: sel.endpoints.length > 1 ? `Then: ${sel.endpoints.slice(1).join(', ')}` : 'No fallbacks',
      rationale: sel.rationale,
      empty: null,
      stale: result.stale ? `Older than ${limit}: Refresh before launching with it.` : null,
    }
  })
}

export interface NitroCard {
  label: string
  lines: string[]
}

/** Nitro's card: an estimate of where OpenRouter will send requests, with Chorus 1's caveats. */
export function nitroCard(view: TiersView | null): NitroCard {
  const nitro = view?.result?.nitro ?? null
  const likely = nitro?.likely ?? null
  const lines = [
    likely
      ? `Likely ${likely.providerName} (${likely.tag}) at ${tps(likely.tpsP50)}: an estimate; actual routing may differ.`
      : 'OpenRouter picks the fastest endpoint for each request.',
  ]
  if (nitro?.likelyFailsRules.length) lines.push(`That endpoint fails: ${nitro.likelyFailsRules.join('; ')}.`)
  lines.push('Requests may be billed at a provider’s priority-tier price. Your account’s guardrails still apply.')
  return { label: 'Nitro', lines }
}

export interface CandidateRow extends EndpointFacts {
  /** Every rule that excludes it, joined; "below the Budget floor" when only the floor does. */
  reasons: string
  /** Excluded from every tier (the floor only affects Budget). */
  excluded: boolean
}

/** Every candidate the ranker considered, eligible or not, in the result's order (by tag). */
export function candidateRows(result: TierResult | null | undefined): CandidateRow[] {
  return (result?.candidates ?? []).map((c) => ({
    tag: c.tag,
    provider: c.providerName,
    speed: tps(c.tpsP50),
    price: price(c.cost?.blended),
    latency: secs(c.latencyP50S),
    uptime: pct(c.uptime1d),
    reasons: c.excludedBy.join('; ') || (c.budgetFloorExcluded ? 'below the Budget floor' : ''),
    excluded: c.excludedBy.length > 0,
  }))
}

export function candidateHeading(rows: CandidateRow[]): string {
  const excluded = rows.filter((r) => r.excluded).length
  return `Every endpoint (${rows.length - excluded} eligible · ${excluded} excluded)`
}

/** RoutingTiers' registry sentence; null when routing knows nothing of the model. */
export function registryLine(view: TiersView | null): string | null {
  const r = view?.registry
  if (!r) return null
  const source = r.source === 'bundled' ? 'Chorus knows this model' : 'Derived from its endpoints'
  return `${source}: at least ${r.nativePrecision ?? 'any precision'}, ${r.minContext.toLocaleString()} tokens of context.`
}

/** When the stored endpoints were fetched; null before the first Refresh. */
export function updatedText(view: TiersView | null, nowMs: number): string | null {
  const at = view?.result?.snapshotFetchedAtMs
  return at ? `Updated ${ago(at, nowMs)}` : null
}

/** Whether Refresh can be pressed, and the title that says why not, or what it costs. */
export function refreshState(view: TiersView | null, busy: boolean): { disabled: boolean; title: string } {
  if (!view) return { disabled: true, title: NO_INPUT }
  if (view.problem) return { disabled: true, title: view.problem }
  if (view.retryInSecs > 0) return { disabled: true, title: `Refreshed less than a minute ago: again in ${view.retryInSecs} s` }
  if (busy) return { disabled: true, title: 'Refreshing…' }
  return { disabled: false, title: REFRESH_TITLE }
}

/** What a finished Refresh did, in RoutingTiers' words. */
export function refreshText(r: RefreshOutcome): string {
  const n = r.probed.length
  if (!n) return 'Refreshed. No cache test needed: every eligible endpoint was tested in the last 14 days.'
  return `Refreshed. Tested prompt caching on ${n} endpoint${n === 1 ? '' : 's'} for ${usd(r.spentUsd)} (at most ${usd(r.estimateUsd)}).`
}

/** The inspector only looks at OpenRouter providers: routing is OpenRouter's. */
export function inspectorProviders(providers: ProviderSummary[]): ProviderSummary[] {
  return providers.filter((p) => p.kind === 'openrouter')
}

/** The provider to start with: the one the observer watches, else the first OpenRouter provider. */
export function defaultProviderId(providers: ProviderSummary[], watchedId: number | null): number | null {
  const usable = inspectorProviders(providers)
  return usable.find((p) => p.id === watchedId)?.id ?? usable[0]?.id ?? null
}

/** No effort, or Chorus's scale (sent as opencode's variant, because effort decides which endpoints qualify). */
export const EFFORT_CHOICES: readonly (Effort | null)[] = [null, 'fast', 'balanced', 'deep', 'max']

export function effortLabel(effort: Effort | null): string {
  return effort === null ? 'No effort' : EFFORT_LABELS[effort]
}

/** The models that have stored routing data, offered while typing a model. */
export function modelSuggestions(observer: ObserverView | null): string[] {
  return observer?.models.map((m) => m.model) ?? []
}
