/** Wording for the project page's hand-off cards (P18). Pure, so it is unit-tested. */
import type { HandoffEntry, HandoffFile, HandoffReturn, IgnoreState, MausActivity } from '../../ipc'

/** "Updated 3 min ago", from the note's UTC `updated:` line. */
export function updatedLabel(iso: string | null, nowMs: number): string {
  if (!iso) return 'Not written yet'
  const at = Date.parse(iso)
  if (Number.isNaN(at)) return `Updated ${iso}`
  const minutes = Math.floor((nowMs - at) / 60_000)
  if (minutes < 1) return 'Updated just now'
  if (minutes < 60) return `Updated ${minutes} min ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `Updated ${hours} h ago`
  return `Updated ${new Date(at).toLocaleDateString([], { day: 'numeric', month: 'short' })}`
}

/** Whether git ignores the note, in a few words. */
export function ignoreLabel(state: IgnoreState | null): string | null {
  switch (state) {
    case 'ignored':
      return 'ignored by git'
    case 'not-ignored':
      return 'not ignored by git yet'
    case 'not-a-repo':
      return 'not a git repository'
    default:
      return null
  }
}

export type EntryRow = { label: string; value: string }

/** An entry's labelled lines that exist, in the note's order. */
export function entryRows(e: HandoffEntry): EntryRow[] {
  const rows: EntryRow[] = []
  if (e.done) rows.push({ label: 'Done', value: e.done })
  if (e.state) rows.push({ label: 'State', value: e.state })
  if (e.open) rows.push({ label: 'Open', value: e.open })
  if (e.next) rows.push({ label: 'Next', value: e.next })
  return rows
}

/** Who wrote an entry, for its badge; nothing for a hand-written one. */
export function authorLabel(e: HandoffEntry): string {
  return e.author === 'chorus' ? 'Chorus' : e.author === 'mausbot' ? 'MausBot' : ''
}

/** "Show 3 earlier entries". */
export function earlierLabel(count: number, open: boolean): string {
  if (open) return 'Hide earlier entries'
  return `Show ${count} earlier ${count === 1 ? 'entry' : 'entries'}`
}

/** What MausBot is doing with the hand-off, from its conversation. */
export function activityLabel(a: MausActivity | null, nowMs: number): string {
  if (!a) return 'Chorus hasn’t asked MausBot yet'
  if (a.problem) return 'MausBot isn’t answering'
  if (!a.found) return 'Chorus can’t see the conversation in MausBot yet'
  if (a.busy) return 'MausBot is working'
  if (a.updatedAtMs === null) return 'MausBot is idle'
  return `MausBot last worked ${agoWords(nowMs - a.updatedAtMs)}`
}

function agoWords(ms: number): string {
  const minutes = Math.max(0, Math.floor(ms / 60_000))
  if (minutes < 1) return 'just now'
  if (minutes < 60) return `${minutes} min ago`
  const hours = Math.floor(minutes / 60)
  return hours < 24 ? `${hours} h ago` : `${Math.floor(hours / 24)} d ago`
}

/** The return card's title. */
export function returnTitle(project: string, returnedBy: string): string {
  return returnedBy === 'mausbot' ? `MausBot handed ${project} back` : `You took ${project} back`
}

/** Where the returned branch stands against its base. */
export function branchState(r: Pick<HandoffReturn, 'merged' | 'ahead' | 'base'>): string {
  const base = r.base.split(' @ ')[0]
  if (r.merged) return `Merged into ${base}`
  if (r.ahead === 0) return `No commits ahead of ${base}`
  return `Not merged yet: ${r.ahead} ${r.ahead === 1 ? 'commit' : 'commits'} ahead of ${base}`
}

/** A file's lines added and removed, or "binary". */
export function fileStat(f: HandoffFile): string {
  if (f.added === null || f.removed === null) return 'binary'
  return `+${f.added} −${f.removed}`
}

/** "and 3 more commits". */
export function moreLabel(n: number, one: string, many: string): string | null {
  return n > 0 ? `and ${n} more ${n === 1 ? one : many}` : null
}

/** The local time a hand-off started or came back. */
export function clockLabel(ms: number | null): string {
  return ms === null ? '' : new Date(ms).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
}
