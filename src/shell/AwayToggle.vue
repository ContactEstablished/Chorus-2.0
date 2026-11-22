<script setup lang="ts">
/**
 * D135: "I'm away". While it's on, Chorus messages Matthew through MausBot when a session finishes,
 * fails or needs him, or a loop run stops. The tray menu has the same switch; both follow
 * `mausUpdatesChanged`.
 */
import { onMounted, onUnmounted, ref } from 'vue'
import { commands, events, unwrap } from '../ipc'

const away = ref(false)
const busy = ref(false)
const problem = ref<string | null>(null)
let unlisten: (() => void) | null = null

onMounted(async () => {
  away.value = await unwrap(commands.mausbotAway()).catch(() => false)
  unlisten = await events.mausUpdatesChanged.listen(({ payload }) => {
    away.value = payload.away
  })
})
onUnmounted(() => unlisten?.())

async function toggle(): Promise<void> {
  busy.value = true
  problem.value = null
  try {
    const view = await unwrap(commands.mausbotUpdatesChange({ enabled: null, away: !away.value, botId: null }))
    away.value = view.away
    problem.value = view.away && (!view.enabled || view.problem) ? (view.enabled ? view.problem : 'Messages to MausBot are switched off in Settings → MausBot.') : null
  } catch (e) {
    problem.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = false
  }
}

const title = () =>
  problem.value
    ? `Away, but Chorus can't message you: ${problem.value}`
    : away.value
      ? "You're away: Chorus messages you on MausBot when a session finishes, fails or needs you. Click when you're back."
      : "Stepping away? Click, and Chorus messages you on MausBot when a session finishes, fails or needs you."
</script>

<template>
  <button class="away" :class="{ on: away, warn: !!problem }" :disabled="busy" :title="title()" :aria-pressed="away" @click="toggle">
    <span class="dot" aria-hidden="true" />
    <span>{{ away ? 'Away' : 'At desk' }}</span>
  </button>
</template>

<style scoped>
.away {
  align-self: center;
  display: flex;
  align-items: center;
  gap: 6px;
  height: 24px;
  margin-right: 6px;
  padding: 0 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--text-muted);
  font-size: 11.5px;
  white-space: nowrap;
}
.away:hover:not(:disabled) {
  color: var(--text-2);
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--text-muted);
}
.away.on {
  border-color: color-mix(in srgb, var(--status-ready) 50%, transparent);
  background: color-mix(in srgb, var(--status-ready) 12%, transparent);
  color: var(--text);
}
.away.on .dot {
  background: var(--status-ready);
}
.away.warn .dot {
  background: var(--status-needs);
}
</style>
