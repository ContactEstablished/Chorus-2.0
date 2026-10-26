<script setup lang="ts">
/**
 * "Are you sure?" before Sessions closes the sessions picked there (D104). It names each one, says
 * what Close does, offers to remove their worktrees where nothing would be lost, then shows each
 * one's outcome as it comes. When all closed with nothing to read, it goes and a notice says so.
 */
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue'
import Icon from '../components/Icon.vue'
import type { SessionSummary } from '../ipc'
import { useSessionsStore } from '../stores/sessions'
import { useNoticesStore } from '../stores/notices'
import { AGENT_CHIPS, ATTENTION } from '../shell/status'
import { closeHeadline, worthShowing, type CloseResult } from '../shell/bulkClose'

const props = defineProps<{ targets: SessionSummary[] }>()
const emit = defineEmits<{ close: [ran: boolean] }>()

const sessions = useSessionsStore()
const notices = useNoticesStore()

const dialog = ref<HTMLDialogElement | null>(null)
const doneButton = ref<HTMLButtonElement | null>(null)
// Focus goes back where it was when the dialog closes.
const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null
const removeWorktrees = ref(false)
const busy = ref(false)
const results = ref<Record<number, CloseResult>>({})
const finished = ref<CloseResult[] | null>(null)

const n = computed(() => props.targets.length)
const plural = computed(() => `${n.value} session${n.value === 1 ? '' : 's'}`)
const live = computed(() => props.targets.filter((s) => s.live).length)
const withWorktree = computed(() => props.targets.filter((s) => s.worktree && !s.worktree.removed).length)
const doneCount = computed(() => Object.keys(results.value).length)
const what = computed(() => {
  const running = !live.value ? '' : live.value === n.value ? (n.value === 1 ? ' It is still running.' : ' All of them are still running.') : ` ${live.value} of them are still running.`
  return `Each one's process ends and it leaves Sessions, as Close does on its own page. This can't be undone: a closed session can't be restarted.${running}`
})
const rows = computed(() => props.targets.map((s) => ({ s, r: results.value[s.id] ?? null })))

onMounted(() => dialog.value?.showModal())
onUnmounted(() => {
  if (opener?.isConnected) opener.focus()
})

function dismiss(): void {
  if (busy.value) return
  dialog.value?.close()
  emit('close', finished.value !== null)
}

function outcome(r: CloseResult): string {
  const spared = r.report?.spared
    ? ` ${r.report.spared} windowed process${r.report.spared === 1 ? ' was' : 'es were'} left open (for example a browser it opened).`
    : ''
  return `Closed.${spared}${r.report?.worktreeNote ? ` ${r.report.worktreeNote}` : ''}`
}

async function confirm(): Promise<void> {
  busy.value = true
  const out = await sessions.closeMany(
    props.targets.map((s) => s.id),
    removeWorktrees.value,
    (r) => (results.value = { ...results.value, [r.id]: r }),
  )
  busy.value = false
  finished.value = out
  if (!worthShowing(out)) {
    notices.info(closeHeadline(out))
    dismiss()
    return
  }
  await nextTick()
  doneButton.value?.focus()
}
</script>

<template>
  <Teleport to="body">
    <dialog ref="dialog" class="close-many" aria-labelledby="close-many-title" @cancel.prevent="dismiss">
      <h2 id="close-many-title">{{ finished ? closeHeadline(finished) : `Close ${plural}?` }}</h2>
      <p v-if="!finished" class="what">{{ what }}</p>
      <ul class="names" :aria-label="finished ? 'Outcome' : 'Sessions to close'">
        <li v-for="{ s, r } in rows" :key="s.id">
          <span class="agent-chip" :class="`agent-${s.agent}`">{{ AGENT_CHIPS[s.agent] }}</span>
          <span class="name" :title="s.cwd">{{ s.name }}</span>
          <span v-if="r?.error !== undefined" class="result">{{ r?.error }}</span>
          <span v-else-if="r" class="result ok"><Icon name="check" :size="13" /> {{ outcome(r) }}</span>
          <span v-else class="state" :class="`st-${s.attention}`">{{ busy ? 'Closing…' : ATTENTION[s.attention].label }}</span>
        </li>
      </ul>
      <label v-if="withWorktree && !finished" class="check">
        <input v-model="removeWorktrees" type="checkbox" :disabled="busy" />
        <span>
          Also remove {{ withWorktree === 1 ? 'the worktree' : `the ${withWorktree} worktrees` }} where nothing is uncommitted and no other
          session works there. Branches are kept.
        </span>
      </label>
      <footer>
        <span v-if="busy" class="progress" role="status">Closing… {{ doneCount }} of {{ n }} done</span>
        <button v-if="finished" ref="doneButton" class="primary" @click="dismiss">Done</button>
        <template v-else>
          <button class="quiet" :disabled="busy" autofocus @click="dismiss">Cancel</button>
          <button class="danger" :disabled="busy" @click="confirm">Close {{ plural }}</button>
        </template>
      </footer>
    </dialog>
  </Teleport>
</template>

<style scoped>
.close-many {
  width: min(600px, calc(100vw - 48px));
  max-height: calc(100dvh - 64px);
  padding: 20px 22px 18px;
  border: 1px solid var(--border-strong);
  border-radius: 14px;
  background: var(--surface-raised);
  color: var(--text);
  box-shadow: 0 24px 80px #0008;
}
.close-many[open] {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.close-many::backdrop {
  background: rgb(0 0 0 / 0.5);
}
h2 {
  margin: 0;
  font-size: 18px;
}
.what {
  margin: 0;
  font-size: 13px;
  line-height: 1.5;
  color: var(--text-2);
}
.names {
  list-style: none;
  margin: 0;
  padding: 0;
  min-height: 0;
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
}
.names li {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) minmax(0, auto);
  align-items: center;
  gap: 10px;
  padding: 7px 12px;
  font-size: 13px;
  background: var(--surface);
}
.names li + li {
  border-top: 1px solid var(--border);
}
.name {
  font-weight: 600;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.state {
  color: var(--st);
  font-size: 12px;
  white-space: nowrap;
}
.state.st-done,
.state.st-running {
  color: var(--text-muted);
}
.result {
  grid-column: 2 / -1;
  font-size: 12px;
  color: var(--status-failed);
  overflow-wrap: anywhere;
}
.result.ok {
  color: var(--text-2);
}
.result.ok :deep(svg) {
  color: var(--status-ready);
  vertical-align: -2px;
}
.check {
  display: flex;
  align-items: flex-start;
  gap: 9px;
  font-size: 13px;
  color: var(--text-2);
  line-height: 1.45;
}
.check input {
  flex: none;
  width: 15px;
  height: 15px;
  margin: 2px 0 0;
  padding: 0;
  accent-color: var(--accent);
}
footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}
.progress {
  margin-right: auto;
  font-size: 12.5px;
  color: var(--text-muted);
}
</style>
