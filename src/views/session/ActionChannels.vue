<script setup lang="ts">
import { ref, watch } from 'vue'
import { commands, type SessionCapabilities } from '../../ipc'

const props = defineProps<{ sessionId: number; live: boolean }>()
const channels = ref<SessionCapabilities | null>(null)
const error = ref('')
let generation = 0
watch(() => [props.sessionId, props.live], async () => {
  const current = ++generation
  channels.value = null
  error.value = ''
  try {
    const result = await commands.sessionActionCapabilities(props.sessionId)
    if (current !== generation) return
    if (result.status === 'error') error.value = result.error
    else channels.value = result.data
  } catch {
    if (current === generation) error.value = 'Action capabilities could not be loaded. Raw remains available.'
  }
}, { immediate: true })

const labels = { prompt: 'Instructions', question: 'Questions', approval: 'Approvals', plan: 'Plan', queue: 'Queue' }
</script>

<template>
  <details class="channels">
    <summary>Friendly capabilities</summary>
    <p v-if="error" role="status">{{ error }}</p>
    <dl v-if="channels">
      <template v-for="(capability, key) in channels.capabilities" :key="key">
        <dt>{{ labels[key] }}</dt>
        <dd>{{ capability.channel === 'unavailable' ? capability.reason : capability.channel === 'held-hook' ? 'Live requests can be answered in Friendly while their hook is connected. Expired or unsupported requests remain in Raw.' : 'Instructions can be delivered after a confirmed pause. The composer is not available yet.' }}</dd>
      </template>
    </dl>
    <p>The Raw terminal remains the control surface for this running session.</p>
  </details>
</template>

<style scoped>
.channels { border-top: 1px solid var(--border); padding: 16px 0; font-size: 12px; color: var(--text-muted); }
summary { cursor: pointer; color: var(--text-2); }
dt { margin-top: 12px; color: var(--text-2); font-weight: 600; }
dd { margin: 4px 0 0; line-height: 1.5; }
p { line-height: 1.5; }
</style>
