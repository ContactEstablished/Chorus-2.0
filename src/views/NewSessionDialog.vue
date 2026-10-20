<script setup lang="ts">
/**
 * New session, in the shell: the launch form in a dialog. Starting opens the first session; any
 * that couldn't start are reported.
 */
import { nextTick, onMounted, onUnmounted, ref } from 'vue'
import NewSessionForm from './newSession/NewSessionForm.vue'
import type { LaunchRequest } from '../ipc'
import { useSessionsStore } from '../stores/sessions'
import { useKeysStore } from '../stores/keys'
import { useViewStore } from '../stores/view'
import { useNoticesStore } from '../stores/notices'
import { AGENT_LABELS } from '../raw/labels'

const sessions = useSessionsStore()
const keys = useKeysStore()
const view = useViewStore()
const notices = useNoticesStore()

const busy = ref(false)
const dialog = ref<HTMLElement | null>(null)
// Focus goes to the first field, and back where it was when the dialog closes.
const before = document.activeElement instanceof HTMLElement ? document.activeElement : null
// Opened over a project's Overview, the session starts in that project.
const initialProject = view.name === 'overview' ? view.projectFilter : null

function close(): void {
  view.newSessionOpen = false
}

function onKey(e: KeyboardEvent): void {
  if (e.key === 'Escape' && !busy.value) close()
}
onMounted(async () => {
  void keys.load()
  window.addEventListener('keydown', onKey)
  await nextTick()
  dialog.value?.querySelector<HTMLElement>('[role=radio][tabindex="0"], input')?.focus()
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKey)
  if (before?.isConnected && !dialog.value?.isConnected) before.focus()
})

async function launch(request: LaunchRequest): Promise<void> {
  busy.value = true
  const out = await sessions.launch(request)
  busy.value = false
  if (!out) return // The store's error is shown as a notice.
  const first = out[0]?.session
  const failed = out.filter((o) => o.error)
  if (failed.length) {
    notices.error(
      `Started ${out.length - failed.length} of ${out.length}.`,
      failed.map((o) => `${AGENT_LABELS[o.agent]} couldn't start: ${o.error}`).join(' '),
    )
  }
  close()
  if (first) view.openSession(first.id)
}
</script>

<template>
  <div class="backdrop" @mousedown.self="!busy && close()">
    <div ref="dialog" class="dialog" role="dialog" aria-modal="true" aria-label="New session">
      <NewSessionForm :keys="keys.keys" :busy="busy" :initial-project="initialProject" @launch="launch" @cancel="close" />
    </div>
  </div>
</template>

<style scoped>
.backdrop {
  position: fixed;
  inset: 0;
  z-index: 40;
  display: grid;
  place-items: center;
  padding: 24px;
  background: rgb(0 0 0 / 0.5);
  animation: fade 150ms ease both;
}
@keyframes fade {
  from {
    opacity: 0;
  }
}
.dialog {
  position: relative;
  width: min(880px, 100%);
  max-height: calc(100vh - 48px);
  overflow: hidden;
  border-radius: 16px;
  background: var(--surface-raised);
  border: 1px solid var(--border-strong);
  box-shadow: 0 24px 60px rgb(0 0 0 / 0.5);
  animation: rise 250ms ease both;
}
@keyframes rise {
  from {
    opacity: 0;
    transform: translateY(10px);
  }
}
</style>
