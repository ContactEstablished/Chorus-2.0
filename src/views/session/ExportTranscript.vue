<script setup lang="ts">
/**
 * "Export transcript…" (P7): the session's recorded history as Markdown, saved where the user
 * picks in Windows' save dialog (opened by the back end). Optionally the agent's own transcript
 * file goes beside it, scrubbed. Both are redacted of secrets.
 */
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { commands } from '../../ipc'
import { useNoticesStore } from '../../stores/notices'

const props = defineProps<{ sessionId: number }>()
const notices = useNoticesStore()
const open = ref(false)
const withTranscript = ref(false)
const busy = ref(false)
const root = ref<HTMLElement | null>(null)

async function run(): Promise<void> {
  busy.value = true
  try {
    const res = await commands.sessionExport(props.sessionId, withTranscript.value)
    if (res.status === 'error') {
      notices.error('Could not export the transcript.', res.error)
      return
    }
    open.value = false
    if (!res.data) return
    const turns = res.data.turns === 1 ? '1 turn' : `${res.data.turns} turns`
    notices.success(`Exported ${turns} to ${res.data.path}`, res.data.transcriptPath ? `Agent transcript: ${res.data.transcriptPath}` : null)
    if (res.data.transcriptNote) notices.info(res.data.transcriptNote)
  } finally {
    busy.value = false
  }
}

function onDocument(e: Event): void {
  if (open.value && root.value && e.target instanceof Node && !root.value.contains(e.target)) open.value = false
}
onMounted(() => document.addEventListener('pointerdown', onDocument, true))
onBeforeUnmount(() => document.removeEventListener('pointerdown', onDocument, true))
</script>

<template>
  <span ref="root" class="export">
    <button class="quiet" :aria-expanded="open" title="Save this session's prompts, tool calls, questions and final messages as Markdown" @click="open = !open">
      Export…
    </button>
    <div v-if="open" class="pop" role="dialog" aria-label="Export transcript">
      <p class="what">Prompts, tool calls (tool and target), questions with their answers and final messages, as Markdown. Secrets are redacted.</p>
      <label class="check">
        <input v-model="withTranscript" type="checkbox" />
        <span>Also save the agent's own transcript file beside it, scrubbed</span>
      </label>
      <div class="row">
        <button class="primary" :disabled="busy" @click="run">{{ busy ? 'Exporting…' : 'Choose where to save…' }}</button>
        <button class="quiet" @click="open = false">Cancel</button>
      </div>
    </div>
  </span>
</template>

<style scoped>
.export {
  position: relative;
}
.pop {
  position: absolute;
  top: calc(100% + 6px);
  right: 0;
  z-index: 40;
  width: 320px;
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  background: var(--surface-raised);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-card);
  box-shadow: 0 12px 32px rgb(0 0 0 / 0.28);
  font-size: 12.5px;
}
.what {
  margin: 0;
  color: var(--text-2);
}
.check {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  color: var(--text-2);
}
.row {
  display: flex;
  gap: 8px;
}
</style>
