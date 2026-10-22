<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue'
import { commands, unwrap, type Snapshot } from '../../ipc'
import GitSummary from '../../components/GitSummary.vue'

const props = defineProps<{ sessionId: number }>()
const status = ref<Snapshot | null>(null)
const error = ref<string | null>(null)
const busy = ref(false)
const checked = ref(false)
let disposed = false
let timer: ReturnType<typeof setTimeout>
async function refresh(): Promise<void> {
  if (busy.value || disposed) return
  const id = props.sessionId
  busy.value = true
  try {
    const next = await unwrap(commands.sessionGitStatus(id))
    if (!disposed && id === props.sessionId) { status.value = next; error.value = null; checked.value = true }
  } catch (e) {
    if (!disposed && id === props.sessionId) { status.value = null; error.value = String(e); checked.value = true }
  } finally { busy.value = false }
}
watch(() => props.sessionId, () => { status.value = null; checked.value = false; error.value = null; void refresh() }, { immediate: true })
async function tick(): Promise<void> {
  if (document.visibilityState === 'visible') await refresh()
  if (!disposed) timer = setTimeout(() => void tick(), 10000)
}
onMounted(() => { timer = setTimeout(() => void tick(), 10000) })
onUnmounted(() => { disposed = true; clearTimeout(timer) })
</script>

<template>
  <div>
    <GitSummary v-if="status" :status="status" />
    <span v-else-if="error" class="problem">{{ error }}</span>
    <span v-else>{{ checked ? 'Not a Git repository' : 'Checking Git…' }}</span>
    <button class="quiet refresh" :disabled="busy" @click="refresh">{{ busy ? 'Checking…' : 'Refresh Git' }}</button>
  </div>
</template>

<style scoped>
.problem { color: var(--status-needs); }
.refresh { padding: 2px 5px; font-size: 11.5px; margin-top: 4px; }
</style>
