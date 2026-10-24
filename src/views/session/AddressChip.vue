<script setup lang="ts">
/**
 * A Claude Code session's address in Claude Code's own peer fleet (P7): the name other Claude
 * sessions reach it by with ListAgents and SendMessage. Verified, changed (sticky until clicked)
 * or unknown, in Chorus 1's words. Chorus passes the session's name as `-n`, so a rename changes
 * the address from the next run.
 */
import { computed, onMounted } from 'vue'
import { useFleetStore } from '../../stores/fleet'

const props = defineProps<{ sessionId: number }>()
const fleet = useFleetStore()
onMounted(() => void fleet.init())

const row = computed(() => fleet.row(props.sessionId))
const state = computed(() => row.value?.address ?? null)
const title = computed(() => {
  const s = state.value
  const how = 'Its address in Claude Code\'s peer fleet (ListAgents, SendMessage). Chorus passes the session name with -n; a rename changes it from the next run.'
  if (!s) return how
  if (s.kind === 'changed' && s.current.trim().toLowerCase() === s.requested.trim().toLowerCase()) {
    return `Another live Claude Code session also answers to ${s.current}, so a message to that name is ambiguous. Rename one of them. Click to acknowledge. ${how}`
  }
  if (s.kind === 'changed') return `Claude Code answers to ${s.current}, not the requested ${s.requested}${s.collision ? ': another live session holds that name' : ''}. Click to acknowledge. ${how}`
  if (s.kind === 'unknown') return `Address unknown: ${s.reason}. ${how}`
  return how
})
</script>

<template>
  <button
    v-if="state"
    class="addr"
    :class="`a-${state.kind}`"
    :title="title"
    :disabled="state.kind !== 'changed'"
    @click="fleet.acknowledge(sessionId)"
  >
    <span class="at">@</span>{{ row?.described }}
  </button>
</template>

<style scoped>
.addr {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  max-width: 280px;
  padding: 1px 8px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--text-2);
  font-family: var(--font-mono);
  font-size: 11.5px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.addr:disabled {
  cursor: default;
  opacity: 1;
}
.at {
  color: var(--text-muted);
}
.a-verified {
  border-color: color-mix(in srgb, var(--status-ready) 50%, transparent);
}
.a-changed {
  border-color: var(--status-needs);
  color: var(--status-needs);
  cursor: pointer;
}
.a-unknown {
  color: var(--text-muted);
  border-style: dashed;
}
</style>
