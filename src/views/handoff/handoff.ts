/** Wording for the project page's hand-off cards (P18). Pure, so it is unit-tested. */
import type { HandoffEntry, IgnoreState } from '../../ipc'

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
