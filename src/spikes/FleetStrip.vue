<script setup lang="ts">
/**
 * Spike S3: the fleet switcher strip's shell. The window is never activated, so it must not take
 * focus from the app you're typing in. A native input shield in the back end receives every press
 * (so WebView2 never activates the window), drags the strip, and reports short clicks here, in
 * physical pixels; this page clicks whatever is under that point. P7 builds the real strip.
 */
import { onMounted, onUnmounted, ref } from 'vue'
import ChorusMark from '../components/ChorusMark.vue'
import { events } from '../ipc'

const clicks = ref(0)
;(window as unknown as { __fleet: { clicks: () => number } }).__fleet = { clicks: () => clicks.value }

let unlisten: (() => void) | null = null
onMounted(async () => {
  unlisten = await events.fleetClick.listen(({ payload }) => {
    const el = document.elementFromPoint(payload.x / devicePixelRatio, payload.y / devicePixelRatio)
    if (el instanceof HTMLElement) el.click()
  })
})
onUnmounted(() => unlisten?.())
</script>

<template>
  <div class="strip">
    <span class="grip" title="Drag to move"><ChorusMark :height="20" /></span>
    <span class="dot needs" /><span class="label">1 needs you</span>
    <span class="dot working" /><span class="label">2 working</span>
    <button class="target" @click="clicks++">Open · {{ clicks }}</button>
  </div>
</template>

<style scoped>
.strip {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 100vh;
  padding: 0 14px;
  background: var(--surface-raised);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-pill);
  user-select: none;
  cursor: grab;
  overflow: hidden;
}
.grip {
  display: flex;
  cursor: grab;
}
.dot {
  width: 9px;
  height: 9px;
  border-radius: 50%;
}
.dot.needs {
  background: var(--status-needs);
}
.dot.working {
  background: var(--status-working);
}
.label {
  color: var(--text-2);
  font-size: 13px;
}
.target {
  margin-left: auto;
  background: var(--accent-soft);
  color: var(--accent);
  border: 1px solid var(--accent-line);
  border-radius: var(--radius-pill);
  padding: 5px 12px;
  font: inherit;
  cursor: pointer;
}
</style>
