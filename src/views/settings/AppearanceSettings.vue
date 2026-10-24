<script setup lang="ts">
/** Theme (D25), text size (the window's zoom) and the Overview's default grouping. */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import ChorusMark from '../../components/ChorusMark.vue'
import { commands, events, type GroupBy, type Theme } from '../../ipc'
import { usePrefsStore, ZOOM_MAX, ZOOM_MIN } from '../../stores/prefs'
import { useNoticesStore } from '../../stores/notices'

const prefs = usePrefsStore()

const THEMES: { key: Theme; name: string; blurb: string; swatches: string[] }[] = [
  { key: 'glacier', name: 'Glacier', blurb: 'Deep blue-black with a cyan accent. The default.', swatches: ['#0a0f16', '#111925', '#1ec8f0', '#ffc400', '#00e676'] },
  { key: 'dusk', name: 'Dusk', blurb: 'Softer slate with a blue accent and gentler status colours.', swatches: ['#1b1e25', '#22262e', '#4f8cff', '#e9b85b', '#86d4a4'] },
]

const GROUPS: { key: GroupBy; label: string }[] = [
  { key: 'status', label: 'Status' },
  { key: 'project', label: 'Project' },
  { key: 'none', label: 'None' },
]

const zoom = computed(() => prefs.prefs.zoom ?? 1)

const notices = useNoticesStore()
async function replay(): Promise<void> {
  const res = await commands.splashReplay(3, false)
  if (res.status === 'error') notices.error("Couldn't open the splash.", res.error)
}

// The fleet strip (P7): shown or hidden here or from the tray's menu; both stay in step.
const strip = ref(true)
let unlistenStrip: (() => void) | null = null
onMounted(async () => {
  strip.value = await commands.fleetShown()
  unlistenStrip = await events.fleetShown.listen(({ payload }) => (strip.value = payload.shown))
})
onUnmounted(() => unlistenStrip?.())
async function setStrip(shown: boolean): Promise<void> {
  strip.value = shown
  await commands.fleetSetShown(shown)
}
</script>

<template>
  <div class="group">
    <h2>Theme</h2>
    <div class="themes">
      <button
        v-for="t in THEMES"
        :key="t.key"
        class="theme"
        :class="{ on: prefs.prefs.theme === t.key }"
        :aria-pressed="prefs.prefs.theme === t.key"
        @click="prefs.update({ theme: t.key })"
      >
        <span class="preview" :data-theme-preview="t.key">
          <ChorusMark :height="18" />
          <span class="swatches">
            <span v-for="c in t.swatches" :key="c" class="sw" :style="{ background: c }" />
          </span>
        </span>
        <span class="name">{{ t.name }}</span>
        <span class="blurb">{{ t.blurb }}</span>
      </button>
    </div>
  </div>

  <div class="group">
    <h2>Text size</h2>
    <p class="hint">Zooms the whole window, terminals included. Ctrl + and Ctrl − change it anywhere but in a terminal; Ctrl 0 resets.</p>
    <div class="zoom">
      <button :disabled="zoom <= ZOOM_MIN" aria-label="Smaller" @click="prefs.zoomBy(-1)">−</button>
      <output>{{ Math.round(zoom * 100) }}%</output>
      <button :disabled="zoom >= ZOOM_MAX" aria-label="Larger" @click="prefs.zoomBy(1)">+</button>
      <button class="quiet" :disabled="zoom === 1" @click="prefs.zoomBy(0)">Reset</button>
    </div>
  </div>

  <div class="group">
    <h2>Start</h2>
    <label class="check">
      <input type="checkbox" :checked="prefs.prefs.splash" @change="prefs.update({ splash: ($event.target as HTMLInputElement).checked })" />
      <span>Play the 3D splash as Chorus starts, with the count of sessions coming back (about 3 seconds; any key skips it)</span>
    </label>
    <button class="quiet replay" @click="replay">Play it now</button>
  </div>

  <div class="group">
    <h2>Fleet strip</h2>
    <label class="check">
      <input type="checkbox" :checked="strip" @change="setStrip(($event.target as HTMLInputElement).checked)" />
      <span>Show the always-on-top strip of sessions that need you. It never takes the focus; drag it anywhere, and click a session to open it here</span>
    </label>
  </div>

  <div class="group">
    <h2>Overview</h2>
    <p class="hint">How the Overview groups session cards when it opens. You can switch there at any time.</p>
    <div class="segmented" role="radiogroup" aria-label="Group sessions by">
      <button
        v-for="g in GROUPS"
        :key="g.key"
        role="radio"
        :aria-checked="prefs.prefs.groupBy === g.key"
        :class="{ on: prefs.prefs.groupBy === g.key }"
        @click="prefs.update({ groupBy: g.key })"
      >
        {{ g.label }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.group {
  padding: 0 0 26px;
  margin-bottom: 26px;
  border-bottom: 1px solid var(--border);
}
.group:last-child {
  border-bottom: 0;
}
h2 {
  margin: 0 0 6px;
  font-size: 17px;
}
.hint {
  margin: 0 0 14px;
  color: var(--text-2);
  font-size: 13px;
}
.themes {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 14px;
  margin-top: 12px;
}
.theme {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  padding: 12px;
  border-radius: var(--radius-card);
  background: var(--surface);
  border: 1px solid var(--border);
  text-align: left;
}
.theme.on {
  border-color: var(--accent);
  box-shadow: 0 0 0 1px var(--accent);
}
.preview {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px;
  border-radius: 10px;
  margin-bottom: 4px;
}
.preview[data-theme-preview='glacier'] {
  background: #0a0f16;
  --accent: #1ec8f0;
  --logo-bar-low: #3e4650;
  --logo-bar-mid: #4a535e;
  --logo-bar-high: #5a646f;
}
.preview[data-theme-preview='dusk'] {
  background: #1b1e25;
  --accent: #4f8cff;
  --logo-bar-low: #444b57;
  --logo-bar-mid: #535b68;
  --logo-bar-high: #646d7b;
}
.swatches {
  display: flex;
  gap: 4px;
}
.sw {
  width: 14px;
  height: 14px;
  border-radius: 4px;
  border: 1px solid rgb(255 255 255 / 0.08);
}
.name {
  font-weight: 600;
}
.blurb {
  font-size: 12.5px;
  color: var(--text-2);
}
.check {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  font-size: 13.5px;
  color: var(--text-2);
  max-width: 60ch;
  line-height: 1.5;
}
.check input {
  margin-top: 3px;
}
.replay {
  margin: 8px 0 0 22px;
  padding: 3px 8px;
  font-size: 12.5px;
}
.zoom {
  display: flex;
  align-items: center;
  gap: 8px;
}
.zoom button:not(.quiet) {
  width: 34px;
  padding: 5px 0;
  font-size: 16px;
}
.zoom output {
  min-width: 52px;
  text-align: center;
  font-variant-numeric: tabular-nums;
  font-weight: 600;
}
.segmented {
  display: inline-flex;
  padding: 3px;
  gap: 2px;
  border-radius: 10px;
  background: var(--surface);
  border: 1px solid var(--border);
}
.segmented button {
  border: 0;
  background: transparent;
  padding: 5px 14px;
  color: var(--text-2);
  font-weight: 500;
}
.segmented button.on {
  background: var(--surface-chip);
  color: var(--text);
  font-weight: 600;
}
</style>
