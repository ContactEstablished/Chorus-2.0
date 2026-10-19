import { defineStore } from 'pinia'
import { ref } from 'vue'
import { commands, events, type AppInfo, type BackendPulse } from '../ipc'

/** Which build is running, where its data lives, and whether the back end is alive. */
export const useAppStore = defineStore('app', () => {
  const info = ref<AppInfo | null>(null)
  const pulse = ref<BackendPulse | null>(null)
  /** Shown on screen when IPC fails (fail visible, D19). */
  const error = ref<string | null>(null)
  let unlisten: (() => void) | null = null

  async function init(): Promise<void> {
    try {
      info.value = await commands.appInfo()
      unlisten = await events.backendPulse.listen((event) => {
        pulse.value = event.payload
      })
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
    }
  }

  function dispose(): void {
    unlisten?.()
    unlisten = null
  }

  return { info, pulse, error, init, dispose }
})
