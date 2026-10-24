<script setup lang="ts">
/**
 * The fleet roster (P7): every open session with its address in Claude Code's peer fleet, or why
 * it has none, and the live Claude Code sessions Chorus didn't start (listed only).
 */
import { onMounted } from 'vue'
import { useFleetStore } from '../../stores/fleet'
import { useViewStore } from '../../stores/view'

const fleet = useFleetStore()
const view = useViewStore()
onMounted(() => void fleet.init())
</script>

<template>
  <details class="roster">
    <summary>Fleet addresses</summary>
    <p v-if="fleet.roster.unreadable" class="warn">{{ fleet.roster.unreadable }} Addresses are unknown until it can be read.</p>
    <ul>
      <li v-for="r in fleet.roster.rows" :key="r.sessionId">
        <button class="quiet link" @click="view.openSession(r.sessionId)">{{ r.name }}</button>
        <span class="addr" :class="r.address ? `a-${r.address.kind}` : 'a-none'">{{ r.described }}</span>
      </li>
    </ul>
    <template v-if="fleet.roster.others.length">
      <p class="sub">Other live Claude Code sessions</p>
      <ul>
        <li v-for="n in fleet.roster.others" :key="n"><span class="addr">{{ n }}</span></li>
      </ul>
    </template>
  </details>
</template>

<style scoped>
.roster {
  font-size: 12.5px;
}
summary {
  cursor: pointer;
  color: var(--text-2);
  font-weight: 600;
}
ul {
  list-style: none;
  margin: 8px 0 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
li {
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.addr {
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-muted);
}
.a-verified {
  color: var(--status-ready);
}
.a-changed {
  color: var(--status-needs);
}
.sub {
  margin: 10px 0 0;
  color: var(--text-muted);
}
.warn {
  color: var(--status-failed);
}
</style>
