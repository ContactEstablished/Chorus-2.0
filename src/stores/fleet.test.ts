import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { FleetRoster } from '../ipc'

type Changed = (event: { payload: { roster: FleetRoster } }) => void

const ipc = vi.hoisted(() => ({
  fleetRoster: vi.fn(),
  fleetAcknowledge: vi.fn(),
  changed: null as Changed | null,
  listens: 0,
}))

vi.mock('../ipc', () => ({
  commands: { fleetRoster: ipc.fleetRoster, fleetAcknowledge: ipc.fleetAcknowledge },
  events: {
    fleetRosterChanged: {
      listen: async (handler: Changed) => {
        ipc.changed = handler
        ipc.listens++
        return () => {}
      },
    },
  },
}))

const { useFleetStore } = await import('./fleet')

const roster = (name: string): FleetRoster => ({
  unreadable: null,
  rows: [{ sessionId: 3, name, address: { kind: 'verified', address: name }, described: name }],
  others: ['Vince'],
})

describe('fleet roster store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    ipc.listens = 0
  })

  it('loads once, follows changes and finds a session row', async () => {
    ipc.fleetRoster.mockResolvedValue(roster('Ada'))
    const s = useFleetStore()
    await s.init()
    await s.init()
    expect(ipc.listens).toBe(1)
    expect(s.row(3)?.described).toBe('Ada')
    expect(s.row(9)).toBeNull()
    ipc.changed!({ payload: { roster: roster('Bob') } })
    expect(s.row(3)?.described).toBe('Bob')
    expect(s.roster.others).toEqual(['Vince'])
  })

  it('acknowledges a changed address through the back end', async () => {
    const s = useFleetStore()
    await s.acknowledge(3)
    expect(ipc.fleetAcknowledge).toHaveBeenCalledWith(3)
  })
})
