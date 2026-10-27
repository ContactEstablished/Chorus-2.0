<script setup lang="ts">
/**
 * The docked composer: one field with its controls on a single row, the queue and receipts folded
 * beneath it. Drafts, the queue and delivery go through the composer store unchanged (D78, D80).
 */
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useComposerStore } from '../../stores/composer'
import { useSessionsStore } from '../../stores/sessions'
import ImageAttachments from '../../components/ImageAttachments.vue'
import QueuedImages from '../../components/QueuedImages.vue'
import { clipboardImages, prepareImage } from '../../components/composerImages'

const props = defineProps<{ sessionId: number }>()
const emit = defineEmits<{ raw: [] }>()
const store = useComposerStore()
const entry = computed(() => store.entry(props.sessionId))
const state = computed(() => entry.value.state)
const fileInput = ref<HTMLInputElement | null>(null)
const uploading = ref(false)
const uploadError = ref('')
const waiting = computed(() => (state.value?.items ?? []).filter(q => q.status === 'queued' || q.status === 'held').sort((a, b) => a.id - b.id))
const recent = computed(() => (state.value?.items ?? []).filter(q => q.status !== 'queued' && q.status !== 'held').slice(0, 5))
const bytes = computed(() => new TextEncoder().encode(entry.value.text).length)
const imageBytes = computed(() => entry.value.images.reduce((total, image) => total + image.bytes, 0))
const imageLimitWarning = computed(() => entry.value.images.length >= 8 || imageBytes.value >= 18 * 1024 * 1024)
const valid = computed(() => entry.value.loaded && (!!entry.value.text.trim() || entry.value.images.length > 0) && bytes.value <= 16384 && !entry.value.busy && !uploading.value)
const imagesDisabled = computed(() => !entry.value.loaded || entry.value.busy || !!entry.value.pending || uploading.value || !state.value || !!state.value.imageReason)
// Clicking a manual Send is the readiness override; automatic queue dispatch still requires Ready.
const canSend = computed(() => !!state.value?.runId && (!state.value.sendReason || (!waiting.value.length && state.value.canConfirmReady)))
const canSendNext = computed(() => !!state.value?.runId && (!state.value.nextSendReason || state.value.canConfirmReady))
// D96: a stopped Claude Code session (an automation's, say) restarts with the draft as its first prompt.
const sessions = useSessionsStore()
const session = computed(() => sessions.byId[props.sessionId])
const restartable = computed(() => !!state.value && !state.value.runId && session.value?.agent === 'claude' &&
  session.value.status === 'exited' && !session.value.live && !session.value.locked)
const sendBlocker = computed(() => {
  if (canSend.value || restartable.value) return null
  const reason = state.value?.sendReason
  return reason === 'Raw input may contain a draft. Check that its prompt is empty before sending.'
    ? 'Send is unavailable right now. Queue this instruction for a later pause.' : reason
})
const labels: Record<string, string> = { queued: 'Queued', held: 'Held for review', cancelled: 'Cancelled', written: 'Delivered · awaiting confirmation', uncertain: 'Delivery uncertain · check Raw', observed: 'Confirmed by agent' }
// The placeholder says what a message does in the session's current state (DESIGN.md).
const PLACEHOLDER: Record<string, string> = {
  'awaiting-approval': 'Or reply to the request in your own words…',
  'needs-answer': 'Type your answer, or ask a follow-up…',
  ready: 'Write your own instruction…',
  working: 'Queue an instruction for the next pause…',
  failed: 'Tell the agent how to recover…',
}
const placeholder = computed(() => {
  const s = session.value
  if (!s) return 'Write an instruction…'
  if (!s.live) return 'Ask a follow-up in the same context…'
  return PLACEHOLDER[s.activity] ?? 'Write an instruction…'
})
const draftStatus = computed(() => entry.value.saving ? 'Saving…' : entry.value.text !== entry.value.saved ? 'Not saved yet' : !entry.value.loaded ? 'Loading draft…' : entry.value.text ? 'Draft saved' : '')
const sendDisabled = computed(() => !valid.value || (!entry.value.pending && !canSend.value))
let timer: ReturnType<typeof setTimeout> | undefined
let disposed = false
let generation = 0
async function poll() {
  clearTimeout(timer)
  const current = ++generation
  await store.refresh(props.sessionId)
  if (!disposed && current === generation) timer = setTimeout(() => void poll(), 1000)
}
watch(() => props.sessionId, () => { void poll() }, { immediate: true })
onBeforeUnmount(() => { disposed = true; generation++; clearTimeout(timer) })
async function send(queue: boolean) { await store.submit(props.sessionId, queue, !queue) }
/** Enter sends, Ctrl+Enter (or Shift+Enter) adds a line. Queueing is the Queue button only (D23: nothing sends itself). */
function keydown(event: KeyboardEvent) {
  if (event.key !== 'Enter' || event.isComposing || event.altKey) return
  if (event.shiftKey) return // native newline
  const area = event.target as HTMLTextAreaElement
  event.preventDefault()
  if (event.ctrlKey || event.metaKey) {
    // Browsers don't insert a newline for Ctrl+Enter, so do it and let the draft store see the edit.
    area.setRangeText('\n', area.selectionStart, area.selectionEnd, 'end')
    area.dispatchEvent(new Event('input', { bubbles: true }))
    return
  }
  if (restartable.value) {
    if (valid.value && entry.value.images.length === 0) void store.restartSend(props.sessionId)
  } else if (!sendDisabled.value) void send(false)
}
async function attach(files: File[]) {
  if (imagesDisabled.value) return
  const id = props.sessionId
  uploading.value = true; uploadError.value = ''
  try {
    for (const file of files) {
      if (store.entry(id).images.length >= 8) throw new Error('A message can have up to 8 images.')
      await store.imageChange(id, 'add', await prepareImage(file))
    }
  } catch (e) { uploadError.value = String(e) }
  finally { uploading.value = false; if (fileInput.value) fileInput.value.value = '' }
}
function paste(event: ClipboardEvent) {
  const files = clipboardImages(event.clipboardData)
  if (!files.length) return // Let ordinary text paste keep its native editing behavior.
  event.preventDefault()
  if (imagesDisabled.value) {
    uploadError.value = state.value?.imageReason ?? 'Please wait for the current draft operation to finish, then paste again.'
    return
  }
  void attach(files)
}
async function removeImage(id: number) {
  uploadError.value = ''
  try { await store.imageChange(props.sessionId, 'remove', id) }
  catch (e) { uploadError.value = String(e) }
}
</script>

<template>
  <section class="composer" aria-label="Friendly prompt composer" tabindex="-1" @paste="paste">
    <label :for="`friendly-prompt-${sessionId}`" class="sr-only">Your instruction</label>
    <div class="field">
      <textarea :id="`friendly-prompt-${sessionId}`" :value="entry.text" :disabled="!entry.loaded || entry.busy || !!entry.pending" rows="2" :placeholder="placeholder" @input="store.edit(sessionId, ($event.target as HTMLTextAreaElement).value)" @keydown="keydown" />
      <ImageAttachments :session-id="sessionId" :images="entry.images" removable :disabled="imagesDisabled" @remove="removeImage" />
      <div class="bar">
        <input ref="fileInput" type="file" accept="image/png,image/jpeg,image/webp" multiple hidden aria-label="Choose images" @change="attach(Array.from(($event.target as HTMLInputElement).files ?? []))">
        <button class="quiet tool" :disabled="imagesDisabled" :title="state?.imageReason || 'Attach PNG, JPEG or WebP images, or paste one'" @click="fileInput?.click()">{{ uploading ? 'Attaching…' : 'Attach images' }}</button>
        <span v-if="imageLimitWarning" class="error" role="status">Limit: 8 images or 20 MB total.</span>
        <span class="status" role="status">{{ draftStatus }}</span>
        <span class="spacer" />
        <button class="tool" :disabled="!valid || !!entry.pending || !state || !!state.queueReason" :title="state?.queueReason || 'Run it at the next confirmed pause'" @click="send(true)">Queue</button>
        <button v-if="restartable" class="primary tool" :disabled="!valid || entry.images.length > 0" @click="store.restartSend(sessionId)">{{ entry.busy ? 'Restarting…' : 'Restart and send' }}</button>
        <button v-else class="primary tool" :disabled="sendDisabled" :title="sendBlocker || 'Send now (Enter)'" @click="send(false)">{{ entry.pending ? 'Retry same request' : 'Send' }}</button>
        <button v-if="entry.error && !entry.pending" class="quiet tool" :disabled="entry.saving || entry.busy" @click="store.reloadDraft(sessionId)">Reload saved draft</button>
      </div>
    </div>
    <p v-if="uploadError" class="error" role="alert">{{ uploadError }}</p>
    <p v-if="bytes > 16384" class="error">This draft exceeds the 16 KiB limit.</p>
    <p v-if="entry.error" class="error" role="alert">{{ entry.error }}</p>
    <p v-if="sendBlocker" class="hint">{{ sendBlocker }} <button class="quiet link" @click="emit('raw')">Open Raw</button></p>
    <p v-if="restartable && entry.images.length" class="hint">Images can't go with Restart and send. Restart the session, then Send.</p>
    <p v-else-if="restartable" class="hint">The session is stopped. Restart and send restarts it in the same conversation, with this as its next message.</p>
    <p v-else-if="state?.queueReason && state.queueReason !== sendBlocker" class="hint">{{ state.queueReason }}</p>
    <details v-if="waiting.length" class="fold queue" open>
      <summary>Queue <span class="n">{{ waiting.length }} / 32</span><span class="sum">runs in order at a confirmed pause</span></summary>
      <p v-if="state?.dispatchReason" class="hint" role="status">{{ state.dispatchReason }}</p>
      <ol>
        <li v-for="(item, index) in waiting" :key="item.id">
          <strong>{{ labels[item.status] }}</strong><p class="instruction">{{ item.text }}</p>
          <QueuedImages :session-id="sessionId" :images="item.images" />
          <p v-if="item.reason" class="hint">{{ item.reason }}</p>
          <div class="buttons">
            <button v-if="index === 0" class="tool" :disabled="entry.busy || !canSendNext" @click="store.change(sessionId, item.id, 'send', true)">Send next now</button>
            <button v-if="item.status === 'held'" class="tool" :disabled="entry.busy || !state?.runId || !!state?.queueReason" @click="store.change(sessionId, item.id, 'resume')">Resume in current run</button>
            <button class="quiet tool" :disabled="entry.busy" @click="store.change(sessionId, item.id, 'cancel')">Cancel</button>
          </div>
        </li>
      </ol>
      <p class="hint">Questions and approvals pause the queue. After a restart, review held instructions before resuming.</p>
    </details>
    <details v-if="recent.length" class="fold receipts">
      <summary>Recent delivery status</summary>
      <ul><li v-for="item in recent" :key="item.id"><strong>{{ labels[item.status] ?? item.status }}</strong><p class="instruction">{{ item.text }}</p><QueuedImages :session-id="sessionId" :images="item.images" /></li></ul>
      <p class="hint">Delivered means written to the terminal. Confirmation comes from the agent. Uncertain deliveries are never retried automatically.</p>
    </details>
  </section>
</template>

<style scoped>
.composer { display: grid; grid-template-columns: minmax(0, 1fr); gap: 6px; }
.field { padding: 8px 8px 8px 12px; border: 1px solid var(--border-strong); border-radius: 14px; background: var(--surface-raised); transition: border-color 0.15s; }
.field:focus-within { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
textarea { display: block; width: 100%; min-height: 46px; max-height: 240px; field-sizing: content; resize: none; padding: 6px 4px; border: 0; background: transparent; color: var(--text); font: inherit; font-size: 14px; line-height: 1.6; }
textarea:focus { outline: none; }
textarea::placeholder { color: var(--text-muted); }
.bar { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; margin-top: 4px; }
.tool { padding: 5px 12px; font-size: 12.5px; }
.status { font-size: 11.5px; color: var(--text-muted); }
.spacer { flex: 1; }
.hint { margin: 0 4px; color: var(--text-muted); font-size: 12px; line-height: 1.5; }
.link { padding: 0 4px; font-size: 12px; color: var(--accent); }
.error { margin: 0 4px; color: var(--status-failed); font-size: 12.5px; }
.fold { margin: 0 4px; font-size: 12.5px; }
.fold summary { cursor: pointer; color: var(--text-2); padding: 4px 0; }
.fold .n { margin-left: 6px; color: var(--text-muted); }
.fold .sum { margin-left: 8px; color: var(--text-muted); font-weight: 400; }
.queue summary { font-weight: 600; }
ol, ul { margin: 6px 0; padding-left: 20px; }
li { margin: 10px 0; font-size: 12px; }
.instruction { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 140px; overflow: auto; font-size: 13px; line-height: 1.55; margin: 4px 0; }
.buttons { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 6px; }
.sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
</style>
