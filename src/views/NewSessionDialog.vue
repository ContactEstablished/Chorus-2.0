<script setup lang="ts">
/**
 * New session, in the shell: the launch form (agent, folder, options, provider, key, preset) in a
 * dialog, plus which project the session joins. Starting opens the first session; any that
 * couldn't start are reported.
 */
import { nextTick, onMounted, onUnmounted, ref } from 'vue'
import Icon from '../components/Icon.vue'
import NewSessionForm from '../raw/NewSessionForm.vue'
import type { LaunchRequest } from '../ipc'
import { useSessionsStore } from '../stores/sessions'
import { useKeysStore } from '../stores/keys'
import { useProjectsStore } from '../stores/projects'
import { useViewStore } from '../stores/view'
import { useNoticesStore } from '../stores/notices'
import { AGENT_LABELS } from '../raw/labels'

const sessions = useSessionsStore()
const keys = useKeysStore()
const projects = useProjectsStore()
const view = useViewStore()
const notices = useNoticesStore()

const busy = ref(false)
const dialog = ref<HTMLElement | null>(null)
// Focus goes to the first field, and back where it was when the dialog closes.
const before = document.activeElement instanceof HTMLElement ? document.activeElement : null
/** null: the project whose folder holds the session's folder, if any. */
const projectId = ref<number | null>(view.name === 'overview' ? view.projectFilter : null)

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
  dialog.value?.querySelector<HTMLElement>('.panel select, .panel input')?.focus()
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKey)
  if (before?.isConnected && !dialog.value?.isConnected) before.focus()
})

async function launch(request: LaunchRequest): Promise<void> {
  busy.value = true
  const out = await sessions.launch({ ...request, session: { ...request.session, projectId: projectId.value } })
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
    <div ref="dialog" class="dialog scroll" role="dialog" aria-modal="true" aria-label="New session">
      <div class="top">
        <label class="project">
          <span>Project</span>
          <select v-model="projectId">
            <option :value="null">By folder</option>
            <option v-for="p in projects.usable" :key="p.id" :value="p.id">{{ p.name }}</option>
          </select>
        </label>
        <button class="quiet x" aria-label="Close" title="Close (Esc)" :disabled="busy" @click="close"><Icon name="close" :size="16" /></button>
      </div>
      <NewSessionForm :keys="keys.keys" :busy="busy" @launch="launch" @cancel="close" />
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
  width: min(600px, 100%);
  max-height: calc(100vh - 48px);
  padding: 20px 24px 24px;
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
.top {
  position: absolute;
  top: 16px;
  right: 16px;
  display: flex;
  align-items: center;
  gap: 8px;
  z-index: 1;
}
.project {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12.5px;
  color: var(--text-muted);
}
.project select {
  padding: 4px 8px;
  font-size: 13px;
}
.x {
  padding: 5px;
  display: grid;
}
.dialog :deep(.panel) {
  max-width: none;
  border: 0;
  padding: 0;
  background: transparent;
}
</style>
