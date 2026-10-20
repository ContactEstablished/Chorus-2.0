<script setup lang="ts">
/**
 * The status bar: whether the back end is answering, the fleet in numbers, and which build and data
 * are live (LESSONS L15). The zoom shows when it isn't 100%; clicking it resets.
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useAppStore } from '../stores/app'
import { useSessionsStore } from '../stores/sessions'
import { usePrefsStore } from '../stores/prefs'
import { counts } from './status'

const app = useAppStore()
const sessions = useSessionsStore()
const prefs = usePrefsStore()

// The back end pulses once a second; three missed pulses means it isn't answering.
const now = ref(Date.now())
const lastPulseAt = ref(0)
let timer: ReturnType<typeof setInterval> | null = null
let lastSeq = -1
onMounted(() => {
  timer = setInterval(() => {
    now.value = Date.now()
    const seq = app.pulse?.seq ?? -1
    if (seq !== lastSeq) {
      lastSeq = seq
      lastPulseAt.value = now.value
    }
  }, 1000)
})
onUnmounted(() => {
  if (timer) clearInterval(timer)
})

const alive = computed(() => app.pulse !== null && now.value - lastPulseAt.value < 3500)
const c = computed(() => counts(sessions.list))
const live = computed(() => sessions.list.filter((s) => s.live).length)
const zoom = computed(() => Math.round((prefs.prefs.zoom ?? 1) * 100))
</script>

<template>
  <footer class="statusbar">
    <span class="item" :class="alive ? 'ok' : 'bad'" :title="alive ? 'The back end is answering' : 'No pulse from the back end for a few seconds'">
      <span class="led" />{{ alive ? 'Connected' : app.error ? `Back end unavailable: ${app.error}` : 'Back end not answering' }}
    </span>
    <span class="item">{{ c.total }} session{{ c.total === 1 ? '' : 's' }} · {{ live }} live</span>
    <span v-if="c.needs" class="item needs">{{ c.needs }} need{{ c.needs === 1 ? 's' : '' }} you</span>
    <span class="spacer" />
    <button v-if="zoom !== 100" class="item zoom" title="Reset zoom (Ctrl 0)" @click="prefs.zoomBy(0)">{{ zoom }}%</button>
    <template v-if="app.info">
      <span class="item">{{ app.info.flavor === 'dev' ? 'dev ' : 'v' }}{{ app.info.version }}</span>
      <code class="item path" :title="`${app.info.identifier}\n${app.info.dataDir}`">{{ app.info.dataDir }}</code>
    </template>
  </footer>
</template>

<style scoped>
.statusbar {
  height: var(--statusbar-h);
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 0 14px;
  border-top: 1px solid var(--border);
  background: var(--bg-sidebar);
  color: var(--text-muted);
  font-size: 11.5px;
  white-space: nowrap;
  min-width: 0;
}
.item {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.led {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--status-ready);
}
.bad {
  color: var(--status-failed);
}
.bad .led {
  background: var(--status-failed);
}
.needs {
  color: var(--status-needs);
}
.spacer {
  flex: 1;
}
.zoom {
  padding: 0 6px;
  border: 0;
  background: var(--surface-chip);
  color: var(--text-2);
  font-size: 11px;
}
.path {
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 34ch;
  display: block;
  direction: rtl;
  text-align: left;
}
@media (max-width: 720px) {
  .path {
    display: none;
  }
}
</style>
