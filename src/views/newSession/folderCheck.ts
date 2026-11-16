// What New session says when its folder check can't finish. Pure, so it is unit-tested.
import type { FolderCheck } from '../../ipc'

export type Outcome<T> = { status: 'ok'; data: T } | { status: 'error'; error: string } | Error

/** A rejected call as an Error, so its reason is kept rather than dropped. */
export function asError(e: unknown): Error {
  return e instanceof Error ? e : new Error(String(e))
}

/** Why a call failed: the back end's reason, or what went wrong reaching it. Never a bare "failed". */
export function failure<T>(r: Outcome<T>, what: string): string {
  if (r instanceof Error) return `Could not check ${what}: ${r.message.trim() || 'Chorus did not answer.'}`
  return r.status === 'error' && r.error.trim() ? r.error : `Could not check ${what}.`
}

/**
 * Whether to offer "Check again": there is a folder, no check is running, and the check failed,
 * never ran, or found something the user can fix outside Chorus (a folder that's gone, a drive
 * that's offline, git that didn't answer).
 */
export function canRecheck(folder: string, pending: boolean, check: FolderCheck | null, problem: string | null): boolean {
  if (!folder.trim() || pending) return false
  return !check || !!(problem || check.problem || check.gitNote)
}
