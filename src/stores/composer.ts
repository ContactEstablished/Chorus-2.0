import { defineStore } from 'pinia'
import { reactive } from 'vue'
import { commands, unwrap, type ComposerState, type ComposerSubmit, type ComposerImage, type Draft } from '../ipc'

type Entry = {
  text: string; saved: string; revision: number; loaded: boolean; busy: boolean; saving: boolean
  images: ComposerImage[];
  error: string; state: ComposerState | null; generation: number; pending: ComposerSubmit | null
}

/** Lives beyond a view's lifetime so switching to Raw never abandons an in-flight draft save. */
export const useComposerStore = defineStore('composer', () => {
  const entries = reactive<Record<number, Entry>>({})
  const writes = new Map<number, Promise<void>>()
  function entry(id: number): Entry {
    return entries[id] ??= { text: '', saved: '', images: [], revision: 0, loaded: false, busy: false, saving: false, error: '', state: null, generation: 0, pending: null }
  }
  function adopt(id: number, state: ComposerState, draft = false) {
    const e = entry(id)
    e.state = state
    if (!e.loaded || draft) {
      e.text = e.saved = state.draft.text
      e.images = state.draft.images
      e.revision = state.draft.revision
      e.loaded = true
    }
  }
  async function refresh(id: number) {
    const e = entry(id), generation = e.generation
    try {
      const state = await unwrap(commands.sessionComposer(id))
      if (generation === e.generation) adopt(id, state)
    } catch (error) { if (generation === e.generation) e.error = String(error) }
  }
  function persist(id: number): Promise<void> {
    if (writes.has(id)) return writes.get(id)!
    const e = entry(id)
    if (e.text === e.saved) return Promise.resolve()
    const task = (async () => {
      e.saving = true
      try {
        while (e.text !== e.saved) {
          const text = e.text
          const saved = await unwrap(commands.sessionDraftSave(id, e.revision, text))
          e.saved = saved.text
          e.revision = saved.revision
          e.images = saved.images
          if (e.text === text) e.text = saved.text
        }
      } catch (error) { e.error = String(error); throw error }
      finally { e.saving = false; writes.delete(id) }
    })()
    writes.set(id, task)
    return task
  }
  function edit(id: number, text: string) {
    const e = entry(id)
    if (e.busy || e.pending) return
    e.text = text; e.error = ''
    void persist(id).catch(() => {})
  }
  async function submit(id: number, queue: boolean, confirmReady: boolean) {
    const e = entry(id), snapshot = e.state
    if (e.busy || !snapshot?.runId) return
    e.busy = true; e.error = ''; e.generation++
    try {
      await persist(id)
      e.pending ??= { runId: snapshot.runId, inputEpoch: snapshot.inputEpoch, draftRevision: e.revision, requestId: crypto.randomUUID(), queue, confirmReady }
      adopt(id, await unwrap(commands.sessionComposerSubmit(id, e.pending)), true)
      e.pending = null
    } catch (error) {
      e.error = String(error)
      // Reconcile an uncertain IPC response before offering a new request identity.
      try {
        const state = await unwrap(commands.sessionComposer(id))
        const accepted = !!e.pending && state.items.some(q => q.requestId === e.pending!.requestId)
        adopt(id, state, accepted)
        e.pending = null
        if (accepted) e.error = ''
      } catch { /* Keep the same request for Retry; do not permit a second submit. */ }
    } finally { e.busy = false }
  }
  /** D96: a stopped Claude Code session restarts with the saved draft as its first prompt. */
  async function restartSend(id: number) {
    const e = entry(id)
    if (e.busy || e.pending || e.state?.runId) return
    e.busy = true; e.error = ''; e.generation++
    try {
      await persist(id)
      adopt(id, await unwrap(commands.sessionRestartSend(id, e.revision)), true)
    } catch (error) {
      e.error = String(error)
      // The draft is given back when nothing was sent; show what the back end kept either way.
      try { adopt(id, await unwrap(commands.sessionComposer(id)), true) } catch { /* Keep the local draft. */ }
    } finally { e.busy = false }
  }
  async function change(id: number, item: number, action: 'cancel' | 'resume' | 'send', confirmReady = false) {
    const e = entry(id), snapshot = e.state
    if (e.busy || !snapshot) return
    e.busy = true; e.error = ''; e.generation++
    try {
      const result = action === 'send'
        ? commands.sessionQueueSend(id, item, snapshot.runId!, snapshot.inputEpoch, confirmReady)
        : commands.sessionQueueChange(id, item, action === 'resume' ? snapshot.runId : null)
      adopt(id, await unwrap(result))
    } catch (error) { e.error = String(error); await refresh(id) }
    finally { e.busy = false }
  }
  async function reloadDraft(id: number) {
    const e = entry(id)
    if (e.busy || e.saving || e.pending) return
    e.generation++
    try { adopt(id, await unwrap(commands.sessionComposer(id)), true); e.error = '' }
    catch (error) { e.error = String(error) }
  }
  async function imageChange(id: number, action: 'add' | 'remove', image: { name: string; png: number[] } | number) {
    const e = entry(id)
    if (!e.loaded || e.busy || e.pending) return
    e.busy = true; e.error = ''; e.generation++
    try {
      await persist(id)
      const result = action === 'add' && typeof image !== 'number'
        ? commands.sessionImageAdd(id, e.revision, image.name, image.png)
        : commands.sessionImageRemove(id, e.revision, image as number)
      const draft: Draft = await unwrap(result)
      e.text = e.saved = draft.text; e.revision = draft.revision; e.images = draft.images
    } catch (error) {
      e.error = String(error)
      // Mutation may have committed before the transport failed. Reload only if no local text
      // remains unsaved; never discard a text save failure to recover attachment metadata.
      if (e.text === e.saved) {
        try { adopt(id, await unwrap(commands.sessionComposer(id)), true) } catch { /* Show original error. */ }
      }
      throw error
    } finally { e.busy = false }
  }
  async function addSuggestion(id: number, text: string) {
    const e = entry(id)
    if (!e.loaded) await refresh(id)
    if (!e.loaded || e.busy || e.pending) throw new Error('Wait for the current draft operation to finish.')
    // Preserve local text and images, including an in-flight draft save. This never submits, so it
    // works whenever the draft can be edited, a stopped session's included.
    e.text = e.text.trim() ? `${e.text}\n\n${text}` : text
    e.error = ''
    await persist(id)
  }
  return { entry, refresh, edit, submit, restartSend, change, reloadDraft, imageChange, addSuggestion }
})
