<script setup lang="ts">
// P1 window: a slim bar saying which build and data are live (LESSONS L15) and the bare Raw view.
// The real shell (rail, title bar, Overview) is P4.
import { onMounted, onUnmounted, ref } from 'vue'
import ChorusMark from '../components/ChorusMark.vue'
import RawView from '../raw/RawView.vue'
import { commands } from '../ipc'
import { useAppStore } from '../stores/app'

const app = useAppStore()
const devMessage = ref<string | null>(null)
onMounted(() => void app.init())
onUnmounted(() => app.dispose())

async function splash(reduceMotion: boolean): Promise<void> {
  const res = await commands.spikeSplashOpen(3, reduceMotion)
  devMessage.value = res.status === 'error' ? res.error : null
}

async function fleet(): Promise<void> {
  const res = await commands.spikeFleetOpen()
  devMessage.value = res.status === 'error' ? res.error : null
}
</script>

<template>
  <div class="shell">
    <header class="bar">
      <ChorusMark :height="22" />
      <strong>Chorus 2.0</strong>
      <template v-if="app.info">
        <span class="muted">{{ app.info.version }}</span>
        <span class="flavor" :class="app.info.flavor">{{ app.info.flavor }}</span>
        <code class="path" :title="app.info.identifier">{{ app.info.dataDir }}</code>
      </template>
      <span v-else-if="app.error" class="error">Back end unavailable: {{ app.error }}</span>
      <span class="spacer" />
      <template v-if="app.info?.flavor === 'dev'">
        <span v-if="devMessage" class="error">{{ devMessage }}</span>
        <button class="quiet" @click="splash(false)">Splash</button>
        <button class="quiet" @click="splash(true)">Splash (reduced motion)</button>
        <button class="quiet" @click="fleet()">Fleet strip</button>
      </template>
    </header>
    <RawView v-if="app.info" :dev="app.info.flavor === 'dev'" class="body" />
  </div>
</template>

<style scoped>
.shell {
  height: 100%;
  display: flex;
  flex-direction: column;
}
.bar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 14px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-sidebar);
  min-width: 0;
}
.bar strong {
  font-family: var(--font-heading);
  font-weight: 650;
}
.muted {
  color: var(--text-muted);
}
.spacer {
  flex: 1;
}
.path {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  user-select: all;
  min-width: 0;
}
.flavor {
  padding: 1px 8px;
  border-radius: var(--radius-pill);
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.flavor.dev {
  background: rgb(255 196 0 / 0.12);
  color: var(--status-needs);
}
.flavor.release {
  background: var(--accent-soft);
  color: var(--accent);
}
.error {
  color: var(--status-failed);
}
.body {
  flex: 1;
  min-height: 0;
}
.bar button {
  padding: 4px 10px;
  font-size: 12.5px;
}
</style>
