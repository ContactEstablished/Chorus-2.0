<script setup lang="ts">
import { computed } from 'vue'
import type { ProgressMessage } from '../../ipc'
const props = defineProps<{ messages: ProgressMessage[]; turnId: number | null; finalMessage?: string | null }>()
const visible = computed(() => props.messages.filter(m => m.turnId === props.turnId && m.text !== props.finalMessage))
</script>

<template>
  <article v-for="message in visible" :key="message.id" class="progress-message">
    <h3>Agent <small>{{ turnId === null ? 'Turn not identified' : 'Progress' }}</small></h3>
    <p>{{ message.text }}</p>
  </article>
</template>

<style scoped>
.progress-message { padding: 16px 18px; background: var(--surface-raised); border: 1px solid var(--border); border-radius: 14px; }
h3 { margin: 0 0 8px; font-size: 13px; }
small { color: var(--text-muted); font-weight: 400; margin-left: 8px; }
p { white-space: pre-wrap; overflow-wrap: anywhere; margin: 6px 0; font-size: 14px; line-height: 1.6; }
</style>
