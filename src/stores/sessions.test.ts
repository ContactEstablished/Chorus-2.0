import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { SessionSummary } from '../ipc'

type Updated = (event: { payload: { session: SessionSummary } }) => void

const ipc = vi.hoisted(() => ({
  sessionsList: vi.fn(),
  sessionCreate: vi.fn(),
  sessionClose: vi.fn(),
  sessionSetLocked: vi.fn(),
  updated: null as Updated | null,
}))

vi.mock('../ipc', async (importOriginal) => {
  const real = await importOriginal<typeof import('../ipc')>()
  return {
    unwrap: real.unwrap,
    commands: {
      sessionsList: ipc.sessionsList,
      sessionCreate: ipc.sessionCreate,
      sessionClose: ipc.sessionClose,
      sessionSetLocked: ipc.sessionSetLocked,
    },
    events: {
      sessionUpdated: {
        listen: async (handler: Updated) => {
          ipc.updated = handler
          return () => {}
        },
      },
    },
  }
})

const { useSessionsStore } = await import('./sessions')

function session(id: number, over: Partial<SessionSummary> = {}): SessionSummary {
  return {
    id,
    name: `Shell ${id}`,
    agent: 'shell',
    cwd: 'C:\\work',
    status: 'running',
    live: true,
    pid: 100 + id,
    locked: false,
    note: null,
    credentialLabel: null,
    credentialEnv: null,
    lastExitCode: null,
    lastEnd: null,
    createdAtSecs: 0,
    runStartedAtSecs: 0,
    scrollbackError: null,
    ...over,
  }
}

describe('sessions store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    for (const f of [ipc.sessionsList, ipc.sessionCreate, ipc.sessionClose, ipc.sessionSetLocked]) f.mockReset()
    ipc.updated = null
  })

  it('loads the list, selects the first session and follows updates', async () => {
    ipc.sessionsList.mockResolvedValue({ status: 'ok', data: [session(2), session(1)] })
    const store = useSessionsStore()
    await store.init()
    expect(store.list.map((s) => s.id)).toEqual([1, 2])
    expect(store.selectedId).toBe(1)

    ipc.updated?.({ payload: { session: session(1, { live: false, status: 'exited', lastExitCode: 3 }) } })
    expect(store.selected?.lastExitCode).toBe(3)

    ipc.updated?.({ payload: { session: session(1, { status: 'closed' }) } })
    expect(store.list.map((s) => s.id)).toEqual([2])
    expect(store.selectedId).toBe(2)
  })

  it('shows a refused request instead of swallowing it', async () => {
    ipc.sessionsList.mockResolvedValue({ status: 'ok', data: [session(1, { locked: true })] })
    ipc.sessionClose.mockResolvedValue({ status: 'error', error: 'Shell 1 is locked. Unlock it before you close it.' })
    const store = useSessionsStore()
    await store.init()
    expect(await store.close(1)).toBeNull()
    expect(store.error).toContain('locked')
    expect(store.list).toHaveLength(1)
  })

  it('selects a session it creates', async () => {
    ipc.sessionsList.mockResolvedValue({ status: 'ok', data: [session(1)] })
    ipc.sessionCreate.mockResolvedValue({ status: 'ok', data: session(5) })
    const store = useSessionsStore()
    await store.init()
    await store.create({ agent: 'shell', cwd: 'C:\\work', name: null, credential: null, cols: 120, rows: 30 })
    expect(store.selectedId).toBe(5)
  })
})
