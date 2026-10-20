/**
 * The terminals on screen, by session, so app-level code (the dev API, the palette) can read one
 * without holding a component ref. A session can show in more than one place (the Wall and its own
 * view); the newest registration wins.
 */
export interface TerminalHandle {
  screenText: () => string
  bufferText: () => string
  focus: () => void
}

const open = new Map<number, TerminalHandle[]>()

export function registerTerminal(sessionId: number, handle: TerminalHandle): () => void {
  open.set(sessionId, [...(open.get(sessionId) ?? []), handle])
  return () => {
    const rest = (open.get(sessionId) ?? []).filter((h) => h !== handle)
    if (rest.length) open.set(sessionId, rest)
    else open.delete(sessionId)
  }
}

export function terminalFor(sessionId: number): TerminalHandle | null {
  return open.get(sessionId)?.at(-1) ?? null
}
