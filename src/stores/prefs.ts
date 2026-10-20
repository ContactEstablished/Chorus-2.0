import { defineStore } from 'pinia'
import { ref } from 'vue'
import { commands, unwrap, type Prefs } from '../ipc'
import { useNoticesStore } from './notices'

export const ZOOM_MIN = 0.8
export const ZOOM_MAX = 1.6
export const ZOOM_STEP = 0.1

const DEFAULTS: Prefs = { theme: 'glacier', zoom: 1, groupBy: 'status', sessionStrip: true, splash: true }

/** One zoom step from `zoom`, rounded to the step and kept in range (as the back end does). */
export function stepZoom(zoom: number, steps: number): number {
  const z = Math.round((zoom + steps * ZOOM_STEP) * 10) / 10
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, z))
}

/**
 * Look and layout preferences, stored by the back end (`prefs_get` / `prefs_set`). The theme is
 * applied here as `data-theme` on the root element; the zoom is the webview's own, applied by the
 * back end.
 */
export const usePrefsStore = defineStore('prefs', () => {
  const prefs = ref<Prefs>({ ...DEFAULTS })

  function applyTheme(): void {
    document.documentElement.dataset.theme = prefs.value.theme
  }

  async function load(): Promise<void> {
    try {
      prefs.value = await commands.prefsGet()
    } catch (e) {
      useNoticesStore().error("Couldn't load your display preferences; using the defaults.", e)
    }
    applyTheme()
  }

  async function update(change: Partial<Prefs>): Promise<void> {
    const before = prefs.value
    prefs.value = { ...before, ...change }
    applyTheme()
    try {
      prefs.value = await unwrap(commands.prefsSet(prefs.value))
    } catch (e) {
      prefs.value = before
      applyTheme()
      useNoticesStore().error("Couldn't save that preference.", e)
    }
  }

  const zoomBy = (steps: number) => update({ zoom: steps === 0 ? 1 : stepZoom(prefs.value.zoom ?? 1, steps) })

  return { prefs, load, update, zoomBy }
})
