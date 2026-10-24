<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useComposerStore } from '../../stores/composer'
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
const sendBlocker = computed(() => {
  if (canSend.value) return null
  const reason = state.value?.sendReason
  return reason === 'Raw input may contain a draft. Check that its prompt is empty before sending.'
    ? 'Send is unavailable right now. Queue this instruction for a later pause.' : reason
})
const labels: Record<string, string> = { queued: 'Queued', held: 'Held for review', cancelled: 'Cancelled', written: 'Delivered · awaiting confirmation', uncertain: 'Delivery uncertain · check Raw', observed: 'Confirmed by agent' }
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
    <div class="heading"><h2>Message the agent</h2><span role="status">{{ entry.saving ? 'Saving draft…' : entry.text !== entry.saved ? 'Draft not saved' : entry.loaded ? 'Draft saved' : 'Loading draft…' }}</span></div>
    <label :for="`friendly-prompt-${sessionId}`" class="sr-only">Your instruction</label>
    <textarea :id="`friendly-prompt-${sessionId}`" :value="entry.text" :disabled="!entry.loaded || entry.busy || !!entry.pending" rows="4" placeholder="Write an instruction…" @input="store.edit(sessionId, ($event.target as HTMLTextAreaElement).value)" />
    <ImageAttachments :session-id="sessionId" :images="entry.images" removable :disabled="imagesDisabled" @remove="removeImage" />
    <div class="image-controls">
      <input ref="fileInput" type="file" accept="image/png,image/jpeg,image/webp" multiple hidden aria-label="Choose images" @change="attach(Array.from(($event.target as HTMLInputElement).files ?? []))">
      <button class="quiet" :disabled="imagesDisabled" @click="fileInput?.click()">{{ uploading ? 'Attaching…' : 'Attach images' }}</button>
      <span v-if="imageLimitWarning" class="error" role="status">Limit: 8 images or 20 MB total.</span>
      <span v-if="state?.imageReason" class="hint">{{ state.imageReason }}</span>
    </div>
    <p v-if="uploadError" class="error" role="alert">{{ uploadError }}</p>
    <p v-if="bytes > 16384" class="error">This draft exceeds the 16 KiB limit.</p>
    <p v-if="entry.error" class="error" role="alert">{{ entry.error }}</p>
    <p v-if="sendBlocker" class="hint">{{ sendBlocker }} <button class="quiet" @click="emit('raw')">Open Raw</button></p>
    <div class="buttons">
      <button class="primary" :disabled="!valid || (!entry.pending && !canSend)" @click="send(false)">{{ entry.pending ? 'Retry same request' : 'Send' }}</button>
      <button :disabled="!valid || !!entry.pending || !state || !!state.queueReason" @click="send(true)">Queue</button>
      <button v-if="entry.error && !entry.pending" class="quiet" :disabled="entry.saving || entry.busy" @click="store.reloadDraft(sessionId)">Reload saved draft</button>
    </div>
    <p v-if="state?.queueReason" class="hint">{{ state.queueReason }}</p>
    <div v-if="waiting.length" class="queue">
      <h3>Instruction queue <span>{{ waiting.length }} / 32</span></h3>
      <p class="hint">Runs in order at a confirmed pause. Questions and approvals pause the queue. After a restart, review held instructions before resuming.</p>
      <p v-if="state?.dispatchReason" class="hint" role="status">{{ state.dispatchReason }}</p>
      <ol>
        <li v-for="(item, index) in waiting" :key="item.id">
          <strong>{{ labels[item.status] }}</strong><p class="instruction">{{ item.text }}</p>
          <QueuedImages :session-id="sessionId" :images="item.images" />
          <p v-if="item.reason" class="hint">{{ item.reason }}</p>
          <div class="buttons">
            <button v-if="index === 0" :disabled="entry.busy || !canSendNext" @click="store.change(sessionId, item.id, 'send', true)">Send next now</button>
            <button v-if="item.status === 'held'" :disabled="entry.busy || !state?.runId || !!state?.queueReason" @click="store.change(sessionId, item.id, 'resume')">Resume in current run</button>
            <button class="quiet" :disabled="entry.busy" @click="store.change(sessionId, item.id, 'cancel')">Cancel</button>
          </div>
        </li>
      </ol>
    </div>
    <details v-if="recent.length" class="receipts">
      <summary>Recent delivery status</summary>
      <ul><li v-for="item in recent" :key="item.id"><strong>{{ labels[item.status] ?? item.status }}</strong><p class="instruction">{{ item.text }}</p><QueuedImages :session-id="sessionId" :images="item.images" /></li></ul>
      <p class="hint">Delivered means written to the terminal. Confirmation comes from the agent. Uncertain deliveries are never retried automatically.</p>
    </details>
  </section>
</template>

<style scoped>
.composer { margin: 24px auto; padding: 20px; border: 1px solid var(--border-strong); border-radius: 14px; background: var(--surface-raised); }
.heading { display: flex; gap: 12px; justify-content: space-between; align-items: baseline; }
h2 { font-size: 16px; margin: 0 0 12px; } h3 { font-size: 14px; margin: 18px 0 8px; }
.heading span, h3 span, .hint { color: var(--text-muted); font-size: 12px; line-height: 1.6; }
textarea { box-sizing: border-box; width: 100%; min-height: 110px; resize: vertical; padding: 12px; border: 1px solid var(--border-strong); border-radius: 8px; background: var(--surface); color: var(--text); font: inherit; font-size: 14px; line-height: 1.6; }
textarea:focus { outline: 2px solid var(--accent); outline-offset: 2px; }
.buttons { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 10px; }
.image-controls { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; }
.queue, .receipts { border-top: 1px solid var(--border); margin-top: 18px; padding-top: 12px; }
ol, ul { padding-left: 22px; } li { margin: 14px 0; font-size: 12px; }
.instruction { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 160px; overflow: auto; font-size: 13px; line-height: 1.6; margin: 6px 0; }
.error { color: var(--status-failed); font-size: 13px; } summary { cursor: pointer; font-size: 13px; }
.sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
</style>
