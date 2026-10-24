<script setup lang="ts">
/**
 * The notification centre (P7): a bell with the unread count in the title bar, beside the window
 * buttons where it is always visible, and a panel of approvals, questions and failures, newest
 * first. A click opens the session and marks it read. In-app only (D15).
 */
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import Icon from '../components/Icon.vue'
import type { Notification } from '../ipc'
import { useNotificationsStore } from '../stores/notifications'
import { useNoticesStore } from '../stores/notices'
import { useSessionsStore } from '../stores/sessions'
import { useViewStore } from '../stores/view'
import { useNow } from './now'
import { ago } from './status'

const store = useNotificationsStore()
const sessions = useSessionsStore()
const view = useViewStore()
const notices = useNoticesStore()
const now = useNow()
const open = ref(false)
const root = ref<HTMLElement | null>(null)
const bell = ref<HTMLButtonElement | null>(null)

// Keyboard users land in the panel when it opens: on the newest notification, else Mark all read.
watch(open, async (isOpen) => {
  if (!isOpen) return
  await nextTick()
  const panel = root.value
  ;(panel?.querySelector<HTMLElement>('.panel .item') ?? panel?.querySelector<HTMLElement>('.panel button:not(:disabled)'))?.focus()
})

const badge = computed(() => (store.unread > 99 ? '99+' : String(store.unread)))
const KIND_LABEL: Record<string, string> = { approval: 'Approval', input: 'Question', failed: 'Failed' }

async function choose(n: Notification): Promise<void> {
  open.value = false
  try {
    await store.markRead(n.id)
  } catch (e) {
    notices.error('Could not mark the notification read.', e)
  }
  if (sessions.list.some((s) => s.id === n.sessionId)) view.openSession(n.sessionId)
  else notices.info('That session has been closed.')
}

async function readAll(): Promise<void> {
  try {
    await store.markAllRead()
  } catch (e) {
    notices.error('Could not mark notifications read.', e)
  }
}

function onDocument(e: Event): void {
  if (open.value && root.value && e.target instanceof Node && !root.value.contains(e.target)) open.value = false
}
function onKey(e: KeyboardEvent): void {
  if (open.value && e.key === 'Escape') {
    open.value = false
    e.stopPropagation()
    bell.value?.focus()
  }
}

onMounted(() => {
  void store.init()
  document.addEventListener('pointerdown', onDocument, true)
  document.addEventListener('keydown', onKey, true)
})
onUnmounted(() => {
  document.removeEventListener('pointerdown', onDocument, true)
  document.removeEventListener('keydown', onKey, true)
  store.dispose()
})
</script>

<template>
  <div ref="root" class="centre">
    <button
      ref="bell"
      class="bell"
      :class="{ on: open }"
      :title="store.unread ? `${store.unread} unread notifications` : 'Notifications'"
      :aria-label="store.unread ? `Notifications, ${store.unread} unread` : 'Notifications'"
      aria-haspopup="dialog"
      :aria-expanded="open"
      @click="open = !open"
    >
      <Icon name="bell" :size="16" :stroke="1.5" />
      <span v-if="store.unread" class="count">{{ badge }}</span>
    </button>
    <section v-if="open" class="panel" role="dialog" aria-label="Notifications">
      <header class="head">
        <h2>Notifications</h2>
        <button class="quiet link" :disabled="!store.unread" @click="readAll">Mark all read</button>
      </header>
      <p v-if="store.error" class="empty error">Notifications could not be loaded: {{ store.error }}</p>
      <p v-else-if="!store.items.length" class="empty">Nothing yet. Approvals, questions and failures appear here; they are kept for 30 days.</p>
      <ul v-else class="list">
        <li v-for="n in store.items" :key="n.id">
          <button class="item" :class="[`k-${n.kind}`, { unread: n.readAt === null }]" @click="choose(n)">
            <span class="dot" aria-hidden="true" />
            <span class="text">
              <span class="title">{{ n.title }}</span>
              <span v-if="n.body" class="body">{{ n.body }}</span>
              <span class="meta">{{ KIND_LABEL[n.kind] ?? n.kind }} · {{ ago(Math.floor((n.createdAt ?? 0) / 1000), now) }}</span>
            </span>
          </button>
        </li>
      </ul>
    </section>
  </div>
</template>

<style scoped>
.centre {
  position: relative;
  height: 100%;
  display: flex;
  align-items: center;
}
.bell {
  position: relative;
  display: grid;
  place-items: center;
  width: 34px;
  height: 28px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-2);
  cursor: pointer;
}
.bell:hover,
.bell.on {
  background: var(--surface-raised);
  color: var(--text);
}
.count {
  position: absolute;
  top: 1px;
  right: 1px;
  min-width: 15px;
  height: 15px;
  padding: 0 4px;
  border-radius: var(--radius-pill);
  background: var(--status-failed);
  color: #fff;
  font-size: 9.5px;
  font-weight: 700;
  line-height: 15px;
  text-align: center;
}
.panel {
  position: absolute;
  top: calc(100% + 4px);
  right: 0;
  z-index: 50;
  width: min(380px, 90vw);
  max-height: min(520px, 70vh);
  display: flex;
  flex-direction: column;
  background: var(--surface-raised);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-card);
  box-shadow: 0 12px 32px rgb(0 0 0 / 0.28);
}
.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border);
}
.head h2 {
  margin: 0;
  font-size: 13px;
}
.empty {
  margin: 0;
  padding: 18px 14px;
  color: var(--text-muted);
  font-size: 12.5px;
}
.empty.error {
  color: var(--status-failed);
}
.list {
  list-style: none;
  margin: 0;
  padding: 4px;
  overflow: auto;
}
.item {
  width: 100%;
  display: flex;
  gap: 10px;
  align-items: flex-start;
  padding: 9px 10px;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-2);
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.item:hover {
  background: var(--surface);
}
.item.unread .title {
  color: var(--text);
  font-weight: 650;
}
.dot {
  flex: none;
  width: 8px;
  height: 8px;
  margin-top: 5px;
  border-radius: 50%;
  background: var(--status-needs);
  opacity: 0.35;
}
.item.unread .dot {
  opacity: 1;
}
.k-failed .dot {
  background: var(--status-failed);
}
.text {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.title {
  font-size: 12.5px;
}
.body {
  font-size: 12px;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.meta {
  font-size: 11px;
  color: var(--text-muted);
}
</style>
