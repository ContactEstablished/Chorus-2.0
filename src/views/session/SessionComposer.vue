<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { commands, unwrap, type QueueSnapshot } from '../../ipc'
import { composerDraft, setComposerDraft, stageComposerSuggestion } from './composerDrafts'

const props = defineProps<{ sessionId: number }>()
const emit = defineEmits<{ raw: [] }>()
const state = ref<QueueSnapshot | null>(null)
const error = ref('')
const notice = ref('')
const busy = ref(false)
const text = computed({ get: () => composerDraft(props.sessionId).text, set: value => setComposerDraft(props.sessionId, value) })
const pending = computed(() => state.value?.entries.filter(e => e.status === 'queued').reverse() ?? [])
const history = computed(() => state.value?.entries.filter(e => e.status !== 'queued').slice(0, 8) ?? [])
let generation = 0
let timer: ReturnType<typeof setTimeout> | undefined
let disposed = false
async function load(): Promise<void> {
  clearTimeout(timer)
  const current = ++generation
  try {
    const next = await unwrap(commands.sessionQueue(props.sessionId))
    if (!disposed && current === generation) state.value = next
  } catch (e) { if (!disposed && current === generation) { state.value = null; error.value = String(e) } }
  finally { if (!disposed && current === generation) timer = setTimeout(() => void load(), 1500) }
}
async function submit(queued: boolean): Promise<void> {
  const runId = state.value?.runId
  if (!runId || busy.value) return
  const sessionId = props.sessionId
  const draft = composerDraft(sessionId)
  const submittedText = draft.text
  const requestId = draft.requestId ??= crypto.randomUUID()
  busy.value = true; error.value = ''; notice.value = ''
  try {
    if (queued) {
      await unwrap(commands.enqueueInstruction(sessionId, { runId, requestId, text: submittedText }))
      notice.value = 'Queue request recorded. Its current delivery or cancellation status is shown below.'
    } else {
      const result = await unwrap(commands.sessionAction(sessionId, { runId, requestId, action: { kind: 'prompt', text: submittedText, source: 'composer' } }))
      if (result.kind === 'prompt') notice.value = result.receipt.status === 'observed' ? 'The agent observed this instruction.' : 'Delivery attempted. Check history for the agent acknowledgement; it will not be retried automatically.'
    }
    if (draft.text === submittedText && draft.requestId === requestId) setComposerDraft(sessionId, '')
    await load()
  } catch (e) { error.value = String(e) }
  finally { busy.value = false }
}
async function change(id: number, rearm: boolean): Promise<void> {
  if (busy.value) return
  busy.value = true; error.value = ''
  try {
    await unwrap(commands.changeQueuedInstruction(props.sessionId, id, rearm ? state.value?.runId ?? null : null))
    await load()
  } catch (e) { error.value = String(e) }
  finally { busy.value = false }
}
async function confirmEmpty(): Promise<void> {
  if (!state.value?.runId || busy.value) return
  busy.value = true; error.value = ''
  try { await unwrap(commands.confirmRawPromptEmpty(props.sessionId, state.value.runId)); await load() }
  catch (e) { error.value = String(e) }
  finally { busy.value = false }
}
function setDraft(value: string): void { stageComposerSuggestion(props.sessionId, value) }
defineExpose({ setDraft })
watch(() => props.sessionId, () => { state.value = null; error.value = ''; notice.value = ''; void load() }, { immediate: true })
onBeforeUnmount(() => { disposed = true; generation++; clearTimeout(timer) })
</script>

<template>
  <section class="composer" aria-label="Instructions">
    <h3>Next instruction</h3>
    <textarea v-model="text" aria-label="Instruction text" rows="4" placeholder="Write an instruction. Pasting or pressing Enter does not send it." />
    <p v-if="state?.blockedReason" class="muted">{{ state.blockedReason }}</p>
    <div v-if="state?.runId && !state.rawEmpty" class="raw-check">
      <p>Check that Raw has no unsent draft. Any new Raw input will require another check. Confirming can release queued instructions.</p>
      <button @click="emit('raw')">Open Raw</button>
      <button :disabled="busy" @click="confirmEmpty">I checked: Raw prompt is empty</button>
    </div>
    <p v-if="pending.length" class="muted">Pending instructions run first. Add new instructions to the queue, or cancel the pending entries.</p>
    <div class="buttons">
      <button :disabled="busy || !text.trim() || !state?.runId || !!state.blockedReason || pending.length > 0" @click="submit(false)">Send instruction</button>
      <button :disabled="busy || !text.trim() || !state?.runId" @click="submit(true)">Queue for next pause</button>
    </div>
    <p v-if="notice" role="status">{{ notice }}</p>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <div v-if="pending.length" class="queue">
      <h3>Queued instructions · {{ pending.length }}</h3>
      <p class="muted">In order, one per confirmed pause. Questions and approvals take priority. Restart requires review; attempted deliveries are never replayed.</p>
      <ol>
        <li v-for="item in pending" :key="item.id">
          <p class="instruction">{{ item.text }}</p>
          <p v-if="item.runId !== state?.runId" class="muted">Saved for an earlier run. Review the text before rearming.</p>
          <button v-if="item.runId !== state?.runId && state?.runId" :disabled="busy" @click="change(item.id, true)">Reviewed: queue for this run</button>
          <button :disabled="busy" @click="change(item.id, false)">Cancel instruction</button>
        </li>
      </ol>
    </div>
    <details v-if="history.length"><summary>Recent queue outcomes</summary><ul><li v-for="item in history" :key="item.id"><p class="instruction">{{ item.text }}</p>{{ item.status === 'cancelled' ? 'Cancelled before delivery' : item.deliveryStatus === 'observed' ? 'Observed by agent' : 'Delivery attempted; not automatically retried' }}</li></ul></details>
  </section>
</template>

<style scoped>
.composer { border: 1px solid var(--border); background: var(--surface-raised); border-radius: 14px; padding: 18px; margin-top: 20px; font-size: 13px; }
h3 { margin: 0 0 10px; font-size: 14px; }
textarea { box-sizing: border-box; width: 100%; resize: vertical; padding: 12px; border: 1px solid var(--border-strong); border-radius: 8px; background: var(--surface); color: var(--text); font: inherit; line-height: 1.5; }
.buttons, .raw-check { margin-top: 12px; }
button { margin: 0 8px 6px 0; }
p { line-height: 1.5; }
.muted { color: var(--text-muted); }
.instruction { white-space: pre-wrap; overflow-wrap: anywhere; }
.queue { margin-top: 18px; border-top: 1px solid var(--border); padding-top: 14px; }
li { margin-bottom: 12px; }
.error { color: var(--status-failed); }
summary { cursor: pointer; }
</style>
