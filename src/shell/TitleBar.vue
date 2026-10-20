<script setup lang="ts">
/**
 * The window's own title bar (the native one is off): a drag area, the command palette's button,
 * the dev badge, and Windows' three buttons. Double-click maximises, as on any Windows title bar.
 */
import { onMounted, onUnmounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import Icon from '../components/Icon.vue'
import { useAppStore } from '../stores/app'

defineEmits<{ palette: [] }>()

const app = useAppStore()
const win = getCurrentWindow()
const maximized = ref(false)
let unlisten: (() => void) | null = null

onMounted(async () => {
  maximized.value = await win.isMaximized().catch(() => false)
  unlisten = await win.onResized(async () => {
    maximized.value = await win.isMaximized().catch(() => false)
  })
})
onUnmounted(() => unlisten?.())
</script>

<template>
  <header class="titlebar" data-tauri-drag-region @dblclick.self="win.toggleMaximize()">
    <div class="left" data-tauri-drag-region>
      <span v-if="app.info?.flavor === 'dev'" class="dev" :title="`${app.info.identifier} — dev data, never the installed app's`">dev build</span>
    </div>
    <button class="palette" title="Search sessions and projects, or run a command (Ctrl+K when no terminal has focus)" @click="$emit('palette')">
      <Icon name="search" :size="14" />
      <span>Search or run a command…</span>
      <kbd>Ctrl K</kbd>
    </button>
    <div class="right" data-tauri-drag-region>
      <button class="win" title="Minimise" aria-label="Minimise" @click="win.minimize()"><Icon name="minimize" :size="16" :stroke="1.25" /></button>
      <button class="win" :title="maximized ? 'Restore' : 'Maximise'" :aria-label="maximized ? 'Restore' : 'Maximise'" @click="win.toggleMaximize()">
        <Icon :name="maximized ? 'restore' : 'maximize'" :size="14" :stroke="1.25" />
      </button>
      <button class="win close" title="Close" aria-label="Close" @click="win.close()"><Icon name="close" :size="16" :stroke="1.25" /></button>
    </div>
  </header>
</template>

<style scoped>
.titlebar {
  height: var(--titlebar-h);
  display: grid;
  grid-template-columns: 1fr minmax(0, 420px) 1fr;
  align-items: center;
  gap: 12px;
  padding-left: 14px;
  user-select: none;
  -webkit-user-select: none;
}
.left {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 100%;
  min-width: 0;
}
.dev {
  padding: 1px 8px;
  border-radius: var(--radius-pill);
  background: color-mix(in srgb, var(--status-needs) 12%, transparent);
  color: var(--status-needs);
  font-size: 10.5px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  white-space: nowrap;
}
.palette {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 26px;
  padding: 0 8px 0 10px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 8px;
  color: var(--text-muted);
  font-size: 12.5px;
  min-width: 0;
}
.palette span {
  flex: 1;
  text-align: left;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.palette kbd {
  font-size: 10.5px;
  padding: 1px 6px;
  border-radius: 5px;
  background: var(--surface-chip);
  color: var(--text-2);
}
.palette:hover {
  color: var(--text-2);
}
.right {
  display: flex;
  justify-content: flex-end;
  height: 100%;
}
.win {
  width: 46px;
  height: 100%;
  display: grid;
  place-items: center;
  padding: 0;
  border: 0;
  border-radius: 0;
  background: transparent;
  color: var(--text-2);
}
.win:hover:not(:disabled) {
  background: var(--surface-chip);
  color: var(--text);
}
.win.close:hover:not(:disabled) {
  background: #c42b1c;
  color: #fff;
}
.win:focus-visible {
  outline-offset: -2px;
}
@media (max-width: 720px) {
  .palette span {
    display: none;
  }
}
</style>
