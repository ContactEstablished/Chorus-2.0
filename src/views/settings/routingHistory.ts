/**
 * What Settings → Routing's History shows, as plain functions: the inspected model's speed history
 * (one row per endpoint tag) and the Refresh log (the last 50 Refreshes, for every model). Types
 * only from the IPC layer, so tests run without Tauri.
 */
import type { Result } from '../../ipc'
// These aren't re-exported by the IPC index, which no task of this phase claims.
import type { RefreshLogEntry, RefreshLogView, SpeedHistoryRow } from '../../ipc/bindings'
import { ago, pct, secs, tps, usd } from '../newSession/routingFormat'

/** The texts decisions.md and the spec fix. */
export const SPEED_HELP = "From the endpoint numbers stored for this model, up to the history setting's age."
export const NO_MODEL = 'Inspect a model to see its speed history.'
export const NO_SPEED = 'No speed history for this model yet.'
export const NO_REFRESHES = 'No Refreshes yet.'

/** Under the Refresh log's heading: what the list holds. */
export const LOG_HELP = 'The last 50 Refreshes, for every model, newest first. A failed Refresh is listed with its message.'

export interface SpeedRow {
  tag: string
  samples: number
  /** The newest sample's age: "12 min ago". */
  newest: string
  /** Median speed, "42 tok/s". */
  speed: string
  /** Median latency, "0.42 s". */
  latency: string
  /** The lowest 1-day uptime, "99.62%". */
  uptime: string
  /** "status 2" when the newest sample's status isn't 0; null otherwise. */
  status: string | null
}

/** The speed history, a row per endpoint tag in the order Rust gives (the ranking's candidates, then the rest by name). Unknown is "—". */
export function speedRows(rows: SpeedHistoryRow[], now: number): SpeedRow[] {
  return rows.map((r) => ({
    tag: r.tag,
    samples: r.samples,
    newest: r.newestAtMs === null ? '—' : ago(r.newestAtMs, now),
    speed: tps(r.medianTps),
    latency: secs(r.medianLatencyS),
    uptime: pct(r.lowestUptime1d),
    status: r.newestStatus === null || r.newestStatus === 0 ? null : `status ${r.newestStatus}`,
  }))
}

export interface LogRow {
  when: string
  model: string
  provider: string
  /** "Done", or the failure's message. */
  result: string
  failed: boolean
  endpoints: number
  /** What OpenRouter reported spent, "$0.0123". */
  spent: string
  /** The estimate, "$0.0500". */
  estimate: string
}

/** The Refresh log, newest first as Rust keeps it. A failure shows its message; its figures are the zeros Rust stored. */
export function logRows(entries: RefreshLogEntry[], now: number): LogRow[] {
  return entries.map((e) => ({
    when: e.atMs === null ? '—' : ago(e.atMs, now),
    model: e.model,
    provider: e.providerName,
    result: e.error === null ? 'Done' : e.error.trim() || 'Failed: no reason was given.',
    failed: e.error !== null,
    endpoints: e.endpointsTested,
    spent: usd(e.spentUsd),
    estimate: usd(e.estimateUsd),
  }))
}

/**
 * What stands in for the speed table: nothing inspected, or nothing stored for the model; null when
 * there are rows to show (or the rows haven't loaded, which the section reports itself).
 */
export function speedNote(model: string, rows: SpeedHistoryRow[] | null): string | null {
  if (!model.trim()) return NO_MODEL
  return rows !== null && rows.length === 0 ? NO_SPEED : null
}

/** What stands in for the Refresh log: its problem as Rust words it, or "No Refreshes yet."; null when there are entries. */
export function logNote(log: RefreshLogView | null): string | null {
  if (log === null) return null
  return log.problem ?? (log.entries.length === 0 ? NO_REFRESHES : null)
}

/** What a read came to: the data, or the reason it failed. */
export type Loaded<T> = { ok: true; data: T } | { ok: false; reason: string }

/** Reads one command's answer; a call that throws is a failure too. */
export async function settle<T>(call: () => Promise<Result<T>>): Promise<Loaded<T>> {
  try {
    const r = await call()
    return r.status === 'ok' ? { ok: true, data: r.data } : { ok: false, reason: r.error }
  } catch (e) {
    return { ok: false, reason: String(e) }
  }
}
