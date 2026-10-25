import { beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { ComposerState } from '../ipc'

const ipc = vi.hoisted(() => ({ sessionComposer: vi.fn(), sessionDraftSave: vi.fn(), sessionComposerSubmit: vi.fn(), sessionImageAdd: vi.fn(), sessionImageRemove: vi.fn() }))
vi.mock('../ipc', async (original) => ({ ...(await original<typeof import('../ipc')>()), commands: ipc }))
const { useComposerStore } = await import('./composer')
const snapshot = (text = '', revision = 0): ComposerState => ({ draft: { text, revision, images: [] }, items: [], runId: 1, inputEpoch: 0, sendReason: null, nextSendReason: null, queueReason: null, canConfirmReady: false, dispatchReason: null, imageReason: null })
const ok = <T>(data: T) => ({ status: 'ok', data })
beforeEach(() => { setActivePinia(createPinia()); vi.resetAllMocks() })

it('adds a suggestion after existing draft text, preserves attachments and never submits', async () => {
  const store = useComposerStore()
  const state = snapshot('My existing instruction', 2)
  state.draft.images = [{ id: 7, name: 'image.png', bytes: 100 }]
  ipc.sessionComposer.mockResolvedValue(ok(state))
  ipc.sessionDraftSave.mockImplementation(async (_id, revision, text) => ok({ text, revision: revision + 1, images: state.draft.images }))
  await store.addSuggestion(1, 'Review the changes.')
  expect(store.entry(1).saved).toBe('My existing instruction\n\nReview the changes.')
  expect(store.entry(1).images).toEqual(state.draft.images)
  expect(ipc.sessionComposerSubmit).not.toHaveBeenCalled()
  store.entry(1).busy = true
  await expect(store.addSuggestion(1, 'Second')).rejects.toThrow('Wait')
  expect(store.entry(1).text).not.toContain('Second')
})

it("adds a suggestion to a stopped session's draft, for Restart then Send", async () => {
  // An automation closes its session when the run ends; its suggestions must still be usable.
  const store = useComposerStore()
  const stopped = { ...snapshot('', 0), runId: null, queueReason: 'Session has no live run. Restart it first.' }
  ipc.sessionComposer.mockResolvedValue(ok(stopped))
  ipc.sessionDraftSave.mockImplementation(async (_id, revision, text) => ok({ text, revision: revision + 1, images: [] }))
  await store.addSuggestion(1, 'Make the fix you proposed.')
  expect(store.entry(1).saved).toBe('Make the fix you proposed.')
  expect(ipc.sessionComposerSubmit).not.toHaveBeenCalled()
})

it('serialises changing drafts and continues saving after an already-saved submit', async () => {
  const store = useComposerStore()
  ipc.sessionComposer.mockResolvedValue(ok(snapshot()))
  await store.refresh(1)
  let finish!: (value: unknown) => void
  ipc.sessionDraftSave.mockImplementationOnce(() => new Promise(r => { finish = r }))
  ipc.sessionDraftSave.mockImplementation(async (_id, revision, text) => ok({ text, revision: revision + 1, images: [] }))
  store.edit(1, 'one'); store.edit(1, 'two')
  finish(ok({ text: 'one', revision: 1, images: [] }))
  await vi.waitFor(() => expect(store.entry(1).saved).toBe('two'))
  expect(ipc.sessionDraftSave.mock.calls.map(c => c.slice(1))).toEqual([[0, 'one'], [1, 'two']])
  ipc.sessionComposerSubmit.mockResolvedValue(ok(snapshot('', 3)))
  await store.submit(1, false, false)
  store.edit(1, 'three')
  await vi.waitFor(() => expect(store.entry(1).saved).toBe('three'))
})

it('retains a request identity after ambiguous transport failure and blocks edits until reconciled', async () => {
  const store = useComposerStore()
  ipc.sessionComposer.mockResolvedValueOnce(ok(snapshot('hello', 1))).mockRejectedValue(new Error('offline'))
  await store.refresh(1)
  ipc.sessionComposerSubmit.mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce(ok(snapshot('', 2)))
  await store.submit(1, false, false)
  const request = store.entry(1).pending?.requestId
  expect(request).toBeTruthy()
  store.edit(1, 'duplicate')
  expect(store.entry(1).text).toBe('hello')
  await store.submit(1, false, false)
  expect(ipc.sessionComposerSubmit.mock.calls.map(c => c[1].requestId)).toEqual([request, request])
  expect(store.entry(1).pending).toBeNull()
})

it('a polling refresh cannot overwrite an in-flight local edit', async () => {
  const store = useComposerStore()
  ipc.sessionComposer.mockResolvedValue(ok(snapshot('saved', 1)))
  await store.refresh(1)
  ipc.sessionDraftSave.mockReturnValue(new Promise(() => {}))
  store.edit(1, 'local')
  await store.refresh(1)
  expect(store.entry(1).text).toBe('local')
  expect(store.entry(1).revision).toBe(1)
})

it('finishes saving text before attaching and submits the revision containing both', async () => {
  const store = useComposerStore()
  ipc.sessionComposer.mockResolvedValue(ok(snapshot()))
  await store.refresh(1)
  let finish!: (value: unknown) => void
  ipc.sessionDraftSave.mockImplementationOnce(() => new Promise(r => { finish = r }))
  store.edit(1, 'look')
  const image = { id: 7, name: 'image.png', bytes: 100 }
  ipc.sessionImageAdd.mockResolvedValue(ok({ text: 'look', revision: 2, images: [image] }))
  const adding = store.imageChange(1, 'add', { name: image.name, png: [1, 2, 3] })
  expect(ipc.sessionImageAdd).not.toHaveBeenCalled()
  store.edit(1, 'edit while attaching')
  finish(ok({ text: 'look', revision: 1, images: [] }))
  await adding
  expect(ipc.sessionImageAdd).toHaveBeenCalledWith(1, 1, image.name, [1, 2, 3])
  expect(store.entry(1).images).toEqual([image])
  ipc.sessionComposerSubmit.mockResolvedValue(ok(snapshot('', 3)))
  await store.submit(1, true, false)
  expect(ipc.sessionComposerSubmit.mock.calls[0]![1].draftRevision).toBe(2)
  expect(store.entry(1).images).toEqual([])
})

it('reconciles an attachment accepted before its response was lost', async () => {
  const store = useComposerStore()
  ipc.sessionComposer.mockResolvedValueOnce(ok(snapshot('look', 1)))
  await store.refresh(1)
  const accepted = snapshot('look', 2)
  accepted.draft.images = [{ id: 3, name: 'one.png', bytes: 100 }]
  ipc.sessionImageAdd.mockRejectedValue(new Error('offline'))
  ipc.sessionComposer.mockResolvedValue(ok(accepted))
  await expect(store.imageChange(1, 'add', { name: 'one.png', png: [1] })).rejects.toThrow('offline')
  expect(store.entry(1).images).toEqual(accepted.draft.images)
  expect(store.entry(1).revision).toBe(2)
  expect(ipc.sessionImageAdd).toHaveBeenCalledTimes(1)
})
