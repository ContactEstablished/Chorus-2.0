import { defineStore } from 'pinia'
import { ref } from 'vue'
import { commands, events, unwrap, type HandoffView } from '../ipc'

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/**
 * Each project's hand-off note (P18), as its page shows it. A project is read when its page opens
 * and again whenever the back end says its note changed.
 */
export const useHandoffStore = defineStore('handoff', () => {
  const views = ref(new Map<number, HandoffView>())
  const errors = ref(new Map<number, string>())
  const busy = ref(new Set<number>())
  let unlisten: (() => void) | null = null

  async function init(): Promise<void> {
    if (unlisten) return
    unlisten = await events.handoffChanged.listen(({ payload }) => {
      if (views.value.has(payload.projectId)) void load(payload.projectId)
    })
  }

  async function load(projectId: number): Promise<void> {
    try {
      views.value.set(projectId, await unwrap(commands.handoffView(projectId)))
      errors.value.delete(projectId)
    } catch (e) {
      errors.value.set(projectId, message(e))
    }
  }

  async function refresh(projectId: number): Promise<void> {
    busy.value.add(projectId)
    try {
      views.value.set(projectId, await unwrap(commands.handoffRefresh(projectId)))
      errors.value.delete(projectId)
    } catch (e) {
      errors.value.set(projectId, message(e))
    } finally {
      busy.value.delete(projectId)
    }
  }

  async function openNote(projectId: number): Promise<void> {
    await unwrap(commands.handoffOpenNote(projectId))
  }

  function dispose(): void {
    unlisten?.()
    unlisten = null
  }

  return { views, errors, busy, init, load, refresh, openNote, dispose }
})
