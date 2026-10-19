<script setup lang="ts">
// P0 placeholder window: proves the build, the typed IPC and which data directory is live.
// The real shell (rail, title bar, Overview) is P4.
import { computed, onMounted, onUnmounted } from 'vue'
import ChorusMark from '../components/ChorusMark.vue'
import { useAppStore } from '../stores/app'

const app = useAppStore()
onMounted(() => void app.init())
onUnmounted(() => app.dispose())

const uptime = computed(() => (app.pulse ? `${app.pulse.uptimeSecs} s` : '…'))
</script>

<template>
  <main class="p0">
    <header class="brand">
      <ChorusMark :height="44" />
      <div>
        <h1>Chorus 2.0</h1>
        <p class="sub">Phase 0 scaffold. No real UI yet.</p>
      </div>
    </header>

    <p v-if="app.error" class="error" role="alert">Back end unavailable: {{ app.error }}</p>

    <dl v-if="app.info" class="facts">
      <dt>Version</dt>
      <dd>{{ app.info.version }}</dd>
      <dt>Build</dt>
      <dd>
        <span class="flavor" :class="app.info.flavor">{{ app.info.flavor }}</span>
        <code>{{ app.info.identifier }}</code>
      </dd>
      <dt>Data directory</dt>
      <dd><code class="path">{{ app.info.dataDir }}</code></dd>
      <dt>Back end</dt>
      <dd>
        <span class="dot" :class="{ live: app.pulse }" />
        {{ app.pulse ? `pulse #${app.pulse.seq} · up ${uptime}` : 'waiting for first pulse…' }}
      </dd>
      <dt>Tauri</dt>
      <dd>{{ app.info.tauriVersion }}</dd>
    </dl>
    <p v-else-if="!app.error" class="sub">Connecting to the back end…</p>
  </main>
</template>

<style scoped>
.p0 {
  max-width: 760px;
  margin: 0 auto;
  padding: 56px 32px;
}
.brand {
  display: flex;
  align-items: center;
  gap: 18px;
  margin-bottom: 32px;
}
h1 {
  font-family: var(--font-heading);
  font-weight: 650;
  font-size: 28px;
  margin: 0;
}
.sub {
  color: var(--text-muted);
  margin: 4px 0 0;
}
.facts {
  display: grid;
  grid-template-columns: max-content 1fr;
  gap: 12px 24px;
  margin: 0;
  padding: 24px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
}
dt {
  color: var(--text-muted);
}
dd {
  margin: 0;
  color: var(--text-2);
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}
code {
  font-family: var(--font-mono);
  font-size: 12.5px;
  color: var(--text);
}
.path {
  user-select: all;
  overflow-wrap: anywhere;
}
.flavor {
  padding: 2px 10px;
  border-radius: var(--radius-pill);
  font-size: 12px;
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
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--status-done);
}
.dot.live {
  background: var(--status-ready);
}
.error {
  color: var(--status-failed);
  background: rgb(255 61 90 / 0.12);
  padding: 12px 16px;
  border-radius: var(--radius-control);
}
</style>
