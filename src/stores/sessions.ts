import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  commands,
  events,
  unwrap,
  type CloseReport,
  type CreateSession,
  type LaunchOutcome,
  type LaunchRequest,
  type SessionSummary,
} from '../ipc'

/**
 * Sessions as the back end reports them. The back end owns every session (ARCHITECTURE §1); this
 * store only mirrors its summaries, kept current by `session-updated` events, and forwards requests.
 */
export const useSessionsStore = defineStore('sessions', () => {
  const byId = ref<Record<number, SessionSummary>>({})
  const selectedId = ref<number | null>(null)
  /** The last failed request, shown on screen (fail visible). */
  const error = ref<string | null>(null)
  let unlisten: (() => void) | null = null

  const list = computed(() => Object.values(byId.value).sort((a, b) => a.id - b.id))
  const selected = computed(() => (selectedId.value === null ? null : (byId.value[selectedId.value] ?? null)))

  function apply(s: SessionSummary): void {
    if (s.status === 'closed') {
      const { [s.id]: _gone, ...rest } = byId.value
      byId.value = rest
      if (selectedId.value === s.id) selectedId.value = list.value[0]?.id ?? null
      return
    }
    byId.value = { ...byId.value, [s.id]: s }
  }

  async function init(): Promise<void> {
    unlisten = await events.sessionUpdated.listen((e) => apply(e.payload.session))
    try {
      for (const s of await unwrap(commands.sessionsList())) apply(s)
      selectedId.value ??= list.value[0]?.id ?? null
    } catch (e) {
      fail(e)
    }
  }

  function dispose(): void {
    unlisten?.()
    unlisten = null
  }

  function fail(e: unknown): void {
    error.value = e instanceof Error ? e.message : String(e)
  }

  /** Runs a request; on failure, shows its message and returns null. */
  async function attempt<T>(run: () => Promise<T>): Promise<T | null> {
    error.value = null
    try {
      return await run()
    } catch (e) {
      fail(e)
      return null
    }
  }

  async function create(request: CreateSession): Promise<SessionSummary | null> {
    const s = await attempt(() => unwrap(commands.sessionCreate(request)))
    if (s) {
      apply(s)
      selectedId.value = s.id
    }
    return s
  }

  /** Starts a preset's sessions and selects the first. Each outcome says if its session failed. */
  async function launch(request: LaunchRequest): Promise<LaunchOutcome[] | null> {
    const out = await attempt(() => unwrap(commands.sessionsLaunch(request)))
    for (const o of out ?? []) if (o.session) apply(o.session)
    const first = out?.[0]?.session
    if (first) selectedId.value = first.id
    return out
  }

  async function rename(id: number, name: string): Promise<boolean> {
    const s = await attempt(() => unwrap(commands.sessionRename(id, name)))
    if (s) apply(s)
    return s !== null
  }

  async function setDescription(id: number, text: string | null): Promise<boolean> {
    const s = await attempt(() => unwrap(commands.sessionSetDescription(id, text)))
    if (s) apply(s)
    return s !== null
  }

  async function setProject(id: number, projectId: number | null): Promise<SessionSummary | null> {
    const s = await attempt(() => unwrap(commands.sessionSetProject(id, projectId)))
    if (s) apply(s)
    return s
  }

  async function restart(id: number): Promise<void> {
    const s = await attempt(() => unwrap(commands.sessionRestart(id)))
    if (s) apply(s)
  }

  async function close(id: number): Promise<CloseReport | null> {
    const report = await attempt(() => unwrap(commands.sessionClose(id)))
    if (report) {
      const s = byId.value[id]
      if (s) apply({ ...s, status: 'closed' })
    }
    return report
  }

  async function setLocked(id: number, locked: boolean, pin: string | null = null): Promise<boolean> {
    const s = await attempt(() => unwrap(commands.sessionSetLocked(id, locked, pin)))
    if (s) apply(s)
    return s !== null
  }

  return {
    byId,
    list,
    selected,
    selectedId,
    error,
    init,
    dispose,
    apply,
    create,
    launch,
    rename,
    setDescription,
    setProject,
    restart,
    close,
    setLocked,
  }
})
