<script setup lang="ts">
/**
 * The fleet strip (P7, from spike S3): an always-on-top bar with how many sessions need you, are
 * working or are ready, and a chip per session that needs you, longest waiting first, in its
 * escalation colour. The window is never activated: a native input shield in the back end takes
 * every press, drags the strip and reports short clicks here, in physical pixels, and this page
 * clicks whatever is under that point. Only a chip does anything: it brings the main window
 * forward on that session. Kept small: no Pinia, no main-window code.
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import ChorusMark from '../components/ChorusMark.vue'
import { commands, events, type SessionSummary } from '../ipc'
import { stripView } from './strip'

const sessions = ref(new Map<number, SessionSummary>())
const now = ref(Math.floor(Date.now() / 1000))
const error = ref<string | null>(null)
const opened = ref(0)
const v = computed(() => stripView([...sessions.value.values()], now.value))

async function openSession(id: number): Promise<void> {
  const result = await commands.fleetOpenSession(id)
  if (result.status === 'error') error.value = result.error
  else opened.value++
}

// For runtime checks: what the strip shows, and how many chips were acted on.
;(window as unknown as { __fleet: unknown }).__fleet = { view: () => v.value, opened: () => opened.value }

const unlisten: (() => void)[] = []
let timer: ReturnType<typeof setInterval> | null = null
onMounted(async () => {
  timer = setInterval(() => (now.value = Math.floor(Date.now() / 1000)), 1000)
  unlisten.push(
    await events.sessionUpdated.listen(({ payload }) => {
      const next = new Map(sessions.value)
      if (payload.session.status === 'closed') next.delete(payload.session.id)
      else next.set(payload.session.id, payload.session)
      sessions.value = next
    }),
    // The theme follows Settings while the strip is open.
    await events.prefsChanged.listen(({ payload }) => (document.documentElement.dataset.theme = payload.prefs.theme)),
    await events.fleetClick.listen(({ payload }) => {
      const el = document.elementFromPoint(payload.x / devicePixelRatio, payload.y / devicePixelRatio)
      el?.closest<HTMLElement>('button')?.click()
    }),
  )
  const [prefs, list] = await Promise.all([commands.prefsGet(), commands.sessionsList()])
  document.documentElement.dataset.theme = prefs.theme
  if (list.status === 'ok') sessions.value = new Map(list.data.map((s) => [s.id, s]))
  else error.value = list.error
})
onUnmounted(() => {
  unlisten.forEach((u) => u())
  if (timer) clearInterval(timer)
})
</script>

<template>
  <div class="strip" :title="error ?? undefined">
    <span class="grip" title="Drag to move"><ChorusMark :height="18" /></span>
    <span class="count" :class="{ zero: !v.needs }"><span class="dot needs" />{{ v.needs }} need{{ v.needs === 1 ? 's' : '' }} you</span>
    <span class="count" :class="{ zero: !v.working }"><span class="dot working" />{{ v.working }}</span>
    <span class="count" :class="{ zero: !v.ready }"><span class="dot ready" />{{ v.ready }}</span>
    <span class="chips">
      <button
        v-for="c in v.chips"
        :key="c.id"
        class="chip"
        :class="c.tier ? `tier-${c.tier}` : ''"
        :title="`${c.name} · ${c.waiting} · click to open in Chorus`"
        :data-session="c.id"
        @click="openSession(c.id)"
      >
        <span class="name">{{ c.name }}</span>
      </button>
      <span v-if="v.more" class="more">+{{ v.more }}</span>
      <span v-if="error" class="err">!</span>
    </span>
  </div>
</template>

<style scoped>
.strip {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 100vh;
  padding: 0 10px 0 14px;
  background: var(--surface-raised);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-pill);
  user-select: none;
  cursor: grab;
  overflow: hidden;
  font-size: 12.5px;
}
.grip {
  display: flex;
  flex: none;
}
.count {
  display: flex;
  align-items: center;
  gap: 5px;
  color: var(--text-2);
  white-space: nowrap;
  flex: none;
}
.count.zero {
  color: var(--text-muted);
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}
.dot.needs {
  background: var(--status-needs);
}
.dot.working {
  background: var(--status-working);
}
.dot.ready {
  background: var(--status-ready);
}
.chips {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-left: auto;
  min-width: 0;
}
.chip {
  --tier: var(--status-needs);
  max-width: 110px;
  padding: 4px 10px;
  border: 1px solid color-mix(in srgb, var(--tier) 60%, transparent);
  border-radius: var(--radius-pill);
  background: color-mix(in srgb, var(--tier) 16%, transparent);
  color: var(--text);
  font: inherit;
  cursor: pointer;
}
.chip.tier-urgent,
.chip.tier-stale {
  --tier: var(--status-failed);
}
.chip.tier-pulse {
  animation: pulse 1.6s ease-in-out infinite;
}
.name {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.more {
  color: var(--text-2);
  flex: none;
}
.err {
  color: var(--status-failed);
  font-weight: 700;
}
@keyframes pulse {
  50% {
    background: color-mix(in srgb, var(--tier) 30%, transparent);
  }
}
</style>
