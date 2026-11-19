import { defineStore } from 'pinia'
import { ref } from 'vue'
import { commands, events, unwrap, type HandoffView, type MausBots } from '../ipc'

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/**
 * Each project's hand-off (P18), as its page shows it: the note, the baton and the return. A
 * project is read when its page opens and again whenever the back end says it changed.
 */
export const useHandoffStore = defineStore('handoff', () => {
  const views = ref(new Map<number, HandoffView>())
  const errors = ref(new Map<number, string>())
  /** Projects with a change in flight, and how many: two at once mustn't end each other's busy. */
  const running = ref(new Map<number, number>())
  const busy = { has: (projectId: number) => (running.value.get(projectId) ?? 0) > 0 }
  /** The project whose Hand to MausBot panel is open (the rail's menu can open it). */
  const openHandTo = ref<number | null>(null)
  let unlisten: (() => void) | null = null
  let listening: Promise<void> | null = null

  /** Listens once, however many cards mount at the same moment. */
  function init(): Promise<void> {
    listening ??= events.handoffChanged
      .listen(({ payload }) => {
        if (views.value.has(payload.projectId)) void load(payload.projectId)
      })
      .then((stop) => {
        unlisten = stop
      })
    return listening
  }

  async function load(projectId: number): Promise<void> {
    try {
      views.value.set(projectId, await unwrap(commands.handoffView(projectId)))
      errors.value.delete(projectId)
    } catch (e) {
      errors.value.set(projectId, message(e))
    }
  }

  /** Runs one change for a project, keeping its view and showing its error. Throws nothing. */
  async function act(projectId: number, run: () => Promise<HandoffView>): Promise<boolean> {
    running.value.set(projectId, (running.value.get(projectId) ?? 0) + 1)
    try {
      views.value.set(projectId, await run())
      errors.value.delete(projectId)
      return true
    } catch (e) {
      errors.value.set(projectId, message(e))
      return false
    } finally {
      const left = (running.value.get(projectId) ?? 1) - 1
      if (left > 0) running.value.set(projectId, left)
      else running.value.delete(projectId)
    }
  }

  const refresh = (projectId: number) => act(projectId, () => unwrap(commands.handoffRefresh(projectId)))
  async function start(projectId: number, botId: string | null, task: string): Promise<boolean> {
    const ok = await act(projectId, () => unwrap(commands.handoffStart(projectId, botId, task)))
    // The panel unmounts as soon as the hand-off shows, so it can't close itself.
    if (ok && openHandTo.value === projectId) openHandTo.value = null
    return ok
  }
  const takeBack = (projectId: number) => act(projectId, () => unwrap(commands.handoffTakeBack(projectId)))
  const dismiss = (projectId: number) => act(projectId, () => unwrap(commands.handoffDismiss(projectId)))

  async function openNote(projectId: number): Promise<void> {
    await unwrap(commands.handoffOpenNote(projectId))
  }

  function continueText(projectId: number): Promise<string> {
    return unwrap(commands.handoffContinueText(projectId))
  }

  function bots(projectId: number): Promise<MausBots> {
    return unwrap(commands.mausbotBots(projectId))
  }

  function dispose(): void {
    unlisten?.()
    unlisten = null
    listening = null
  }

  return { views, errors, busy, openHandTo, init, load, refresh, start, takeBack, dismiss, openNote, continueText, bots, dispose }
})
