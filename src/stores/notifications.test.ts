import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { Notification, NotificationAdded } from '../ipc'

type Added = (event: { payload: NotificationAdded }) => void

const ipc = vi.hoisted(() => ({
  notificationsList: vi.fn(),
  notificationRead: vi.fn(),
  notificationsReadAll: vi.fn(),
  added: null as Added | null,
}))

vi.mock('../ipc', async (importOriginal) => {
  const real = await importOriginal<typeof import('../ipc')>()
  return {
    unwrap: real.unwrap,
    commands: {
      notificationsList: ipc.notificationsList,
      notificationRead: ipc.notificationRead,
      notificationsReadAll: ipc.notificationsReadAll,
    },
    events: {
      notificationAdded: {
        listen: async (handler: Added) => {
          ipc.added = handler
          return () => {}
        },
      },
    },
  }
})

const { useNotificationsStore } = await import('./notifications')

function note(id: number, over: Partial<Notification> = {}): Notification {
  return { id, sessionId: 1, kind: 'approval', title: `N${id}`, body: '', createdAt: id * 1000, readAt: null, ...over }
}

describe('notification centre store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    ipc.added = null
  })

  it('loads the history and the unread count', async () => {
    ipc.notificationsList.mockResolvedValue({ status: 'ok', data: { items: [note(2), note(1, { readAt: 5 })], unread: 1 } })
    const s = useNotificationsStore()
    await s.init()
    expect(s.items.map((n) => n.id)).toEqual([2, 1])
    expect(s.unread).toBe(1)
    expect(s.error).toBeNull()
  })

  it('shows why the history could not load', async () => {
    ipc.notificationsList.mockResolvedValue({ status: 'error', error: 'database: locked' })
    const s = useNotificationsStore()
    await s.init()
    expect(s.error).toBe('database: locked')
  })

  it('reads the history again when the back end answers after a failed load', async () => {
    ipc.notificationsList.mockResolvedValueOnce({ status: 'error', error: 'busy' })
    ipc.notificationsList.mockResolvedValueOnce({ status: 'ok', data: { items: [note(1)], unread: 1 } })
    const s = useNotificationsStore()
    await s.init()
    expect(s.error).toBe('busy')
    ipc.added!({ payload: { notification: note(2), unread: 2 } })
    await vi.waitFor(() => expect(s.error).toBeNull())
    expect(ipc.notificationsList).toHaveBeenCalledTimes(2)
  })

  it('puts a new notification first, once, with the back end unread count', async () => {
    ipc.notificationsList.mockResolvedValue({ status: 'ok', data: { items: [note(1)], unread: 1 } })
    const s = useNotificationsStore()
    await s.init()
    ipc.added!({ payload: { notification: note(2), unread: 2 } })
    ipc.added!({ payload: { notification: note(2), unread: 2 } })
    expect(s.items.map((n) => n.id)).toEqual([2, 1])
    expect(s.unread).toBe(2)
  })

  it('marks one read once, and all read', async () => {
    ipc.notificationsList.mockResolvedValue({ status: 'ok', data: { items: [note(2), note(1)], unread: 2 } })
    ipc.notificationRead.mockResolvedValue({ status: 'ok', data: 1 })
    ipc.notificationsReadAll.mockResolvedValue({ status: 'ok', data: null })
    const s = useNotificationsStore()
    await s.init()
    await s.markRead(2)
    await s.markRead(2)
    expect(ipc.notificationRead).toHaveBeenCalledTimes(1)
    expect(s.unread).toBe(1)
    expect(s.items[0]!.readAt).not.toBeNull()
    await s.markAllRead()
    expect(s.unread).toBe(0)
    expect(s.items.every((n) => n.readAt !== null)).toBe(true)
  })
})
