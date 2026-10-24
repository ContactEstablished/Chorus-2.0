import { defineStore } from 'pinia'
import { ref } from 'vue'
import { commands, events, unwrap, type Notification } from '../ipc'

/** Most notifications kept in the panel; the back end lists at most this many too. */
const MAX_SHOWN = 200

/**
 * The notification centre's history (P7): one entry per approval, question or failure, newest
 * first, kept 30 days by the back end. In-app only (D15).
 */
export const useNotificationsStore = defineStore('notifications', () => {
  const items = ref<Notification[]>([])
  const unread = ref(0)
  const error = ref<string | null>(null)
  let unlisten: (() => void) | null = null

  async function init(): Promise<void> {
    if (!unlisten) {
      unlisten = await events.notificationAdded.listen(({ payload }) => {
        items.value = [payload.notification, ...items.value.filter((n) => n.id !== payload.notification.id)].slice(0, MAX_SHOWN)
        unread.value = payload.unread
        // The back end answers again: a list that failed to load is read again.
        if (error.value) void init()
      })
    }
    try {
      const page = await unwrap(commands.notificationsList())
      items.value = page.items
      unread.value = page.unread
      error.value = null
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
    }
  }

  async function markRead(id: number): Promise<void> {
    const n = items.value.find((x) => x.id === id)
    if (!n || n.readAt !== null) return
    unread.value = await unwrap(commands.notificationRead(id))
    n.readAt = Date.now()
  }

  async function markAllRead(): Promise<void> {
    await unwrap(commands.notificationsReadAll())
    const now = Date.now()
    for (const n of items.value) if (n.readAt === null) n.readAt = now
    unread.value = 0
  }

  function dispose(): void {
    unlisten?.()
    unlisten = null
  }

  return { items, unread, error, init, markRead, markAllRead, dispose }
})
