/**
 * How routing numbers and sentences read, shared by New session's tiers and Settings → Routing
 * (Chorus 1's words). Plain functions: types only from the IPC layer, so tests run without Tauri.
 */
import type { CandidateExplanation, TiersView } from '../../ipc'

type TierResult = NonNullable<TiersView['result']>

export function tps(n: number | null | undefined): string {
  return n == null ? '—' : `${Math.round(n)} tok/s`
}
export function price(n: number | null | undefined): string {
  return n == null ? '—' : `$${n >= 1 ? n.toFixed(2) : n.toPrecision(3)}/M`
}
export function pct(n: number | null | undefined): string {
  return n == null ? '—' : `${n.toFixed(2)}%`
}
export function secs(n: number | null | undefined): string {
  return n == null ? '—' : `${n.toFixed(2)} s`
}

/** Money: under $1 with four decimals ($0.0123), otherwise two ($1.23). Unknown is "—", never $0. */
export function usd(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return '—'
  return `$${Math.abs(n) < 1 ? n.toFixed(4) : n.toFixed(2)}`
}

const MINUTE = 60_000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR

/** A past time, relative to `nowMs`: "just now", "12 min ago", "3 h ago", "1 day ago", "4 days ago". */
export function ago(atMs: number, nowMs: number): string {
  const elapsed = nowMs - atMs
  if (elapsed < MINUTE) return 'just now'
  if (elapsed < HOUR) return `${Math.floor(elapsed / MINUTE)} min ago`
  if (elapsed < DAY) return `${Math.floor(elapsed / HOUR)} h ago`
  const days = Math.floor(elapsed / DAY)
  return days === 1 ? '1 day ago' : `${days} days ago`
}

/** A future time, relative to `nowMs`: "in 18 min" (rounded up, at least 1), or "in 2 h" from 60 min up. */
export function inTime(atMs: number, nowMs: number): string {
  const minutes = Math.max(1, Math.ceil((atMs - nowMs) / MINUTE))
  return minutes < 60 ? `in ${minutes} min` : `in ${Math.floor(minutes / 60)} h`
}

// How each tier chose, in Chorus 1's words.
export function why(tier: 'budget' | 'balanced' | 'fast', result: TierResult | null | undefined): string {
  const eligible = (result?.candidates ?? []).filter((c) => c.excludedBy.length === 0)
  const n = eligible.length
  if (tier === 'budget') {
    const above = eligible.filter((c) => !c.budgetFloorExcluded).length
    const floor = result?.budgetFloorTps
    return `First of ${above} endpoints at or above the ${floor == null ? '?' : floor.toFixed(1)} tok/s Budget floor, scored mostly on price.`
  }
  return tier === 'balanced' ? `First of ${n} eligible endpoints, scored equally on price and speed.` : `First of ${n} eligible endpoints, scored on speed only.`
}

export function details(c: CandidateExplanation | undefined): string[] {
  if (!c) return []
  const lines = [
    `${c.tag} (${c.providerName}), ${c.quantization}`,
    `${price(c.cost?.blended)} blended · ${tps(c.tpsP50)} · ${c.effectiveTps == null ? '—' : `${c.effectiveTps.toFixed(1)} effective tok/s`}`,
    `${secs(c.latencyP50S)} latency · ${pct(c.uptime1d)} uptime`,
  ]
  const flags = [c.cost?.cacheCredit ? 'cache verified' : 'cache not verified', c.limitedHistory ? 'limited history' : `${c.observations} observations`]
  lines.push(flags.join(' · '))
  return lines
}

/**
 * Why the tiers can't be chosen: `routing_tiers` failed, so there is no view and its error says
 * why (a refused model, a missing provider). With a view loaded, or nothing failed, nothing is
 * blocked; a failed Refresh keeps the view and its error shows apart from the tiers.
 */
export function tiersBlocked(view: TiersView | null, error: string | null): string | null {
  return view === null && error !== null ? error : null
}
