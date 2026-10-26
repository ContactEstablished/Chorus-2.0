import type { CloseReport, SessionSummary } from '../ipc'

/**
 * Closing many sessions at once from Sessions' selection (D104): which can be picked, how a batch
 * runs, and what its outcome says.
 */

/** How many closes run at once: each can wait seconds for a live agent's processes to end. */
export const CLOSE_AT_ONCE = 4

export type CloseResult = { id: number; name: string } & ({ report: CloseReport; error?: undefined } | { error: string; report?: undefined })

/** A locked session refuses Close, so it can't be picked. */
export function pickable(s: SessionSummary): boolean {
  return !s.locked
}

/** How much of `shown` is picked: drives the "All shown" box (checked, mixed or empty). */
export function pickedState(shown: SessionSummary[], picked: ReadonlySet<number>): 'none' | 'some' | 'all' {
  const can = shown.filter(pickable)
  const n = can.filter((s) => picked.has(s.id)).length
  if (n === 0) return 'none'
  return n === can.length ? 'all' : 'some'
}

/** Runs `task` on each item, at most `limit` at once. Results keep the items' order. */
export async function eachLimited<T, R>(items: readonly T[], limit: number, task: (item: T) => Promise<R>): Promise<R[]> {
  const out: R[] = new Array<R>(items.length)
  let next = 0
  async function worker(): Promise<void> {
    while (next < items.length) {
      const i = next++
      out[i] = await task(items[i] as T)
    }
  }
  await Promise.all(Array.from({ length: Math.min(Math.max(1, limit), items.length) }, worker))
  return out
}

const sessions = (n: number): string => `${n} session${n === 1 ? '' : 's'}`

/** The headline once a batch is done. */
export function closeHeadline(results: CloseResult[]): string {
  const closed = results.filter((r) => r.report).length
  if (closed === results.length) return `Closed ${sessions(closed)}.`
  if (closed === 0) return `Couldn't close ${results.length === 1 ? 'the session' : `any of the ${sessions(results.length)}`}.`
  return `Closed ${closed} of ${sessions(results.length)}.`
}

/** Whether a batch's outcome needs reading: a failure, a worktree note or a process left open. */
export function worthShowing(results: CloseResult[]): boolean {
  return results.some((r) => r.error !== undefined || !!r.report?.worktreeNote || !!r.report?.spared)
}
