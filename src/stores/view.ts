import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

/** The rail's destinations, plus one session's own view. */
export type ViewName = 'overview' | 'sessions' | 'session' | 'grid' | 'bots' | 'automations' | 'loop' | 'council' | 'documents' | 'notes' | 'settings'

export type SettingsSection = 'appearance' | 'projects' | 'providers' | 'routing' | 'keys' | 'agents' | 'narrator' | 'council' | 'phone' | 'mausbot'

interface Place {
  name: ViewName
  sessionId: number | null
}

const MAX_BACK = 50

/**
 * Where the main window is. There is no router (ARCHITECTURE §10); this store is the one place
 * that knows which view is showing, with a short history for Back.
 */
export const useViewStore = defineStore('view', () => {
  const place = ref<Place>({ name: 'overview', sessionId: null })
  const back = ref<Place[]>([])
  /** The Overview's and the Grid's project filter; null is every project. */
  const projectFilter = ref<number | null>(null)
  const settingsSection = ref<SettingsSection>('appearance')
  /** Whether the New session dialog is open. */
  const newSessionOpen = ref(false)
  /**
   * How a project is shown: the Grid of its sessions' live terminals, or the Filmstrip (one session
   * at a time, its project's others in the strip above). In Grid mode a project click opens its Grid.
   */
  const projectLayout = ref<'filmstrip' | 'grid'>('filmstrip')
  /** The command palette (Ctrl+K), or its projects-only form (Ctrl+G). */
  const palette = ref<'all' | 'projects' | null>(null)

  const name = computed(() => place.value.name)
  const sessionId = computed(() => place.value.sessionId)

  function go(to: ViewName, sessionId: number | null = null): void {
    const here = place.value
    if (here.name === to && here.sessionId === sessionId) return
    back.value = [...back.value, here].slice(-MAX_BACK)
    place.value = { name: to, sessionId }
  }

  function openSession(id: number): void {
    go('session', id)
  }

  function openSettings(section: SettingsSection): void {
    settingsSection.value = section
    go('settings')
  }

  /** Shows one project (null: all): its Grid in Grid mode, otherwise the Overview filtered to it. */
  function openProject(id: number | null): void {
    projectFilter.value = id
    go(projectLayout.value === 'grid' ? 'grid' : 'overview')
  }

  /** Shows the Overview of every project, whatever the mode. */
  function openOverview(): void {
    projectFilter.value = null
    go('overview')
  }

  /** Switches to Grid mode and shows one project's Grid (null: every session). */
  function openGrid(id: number | null): void {
    projectLayout.value = 'grid'
    openProject(id)
  }

  /** Switches to Filmstrip mode and opens a session, or the project's Overview when it has none. */
  function openFilmstrip(projectId: number | null, sessionId: number | null): void {
    projectLayout.value = 'filmstrip'
    if (sessionId !== null) openSession(sessionId)
    else openProject(projectId)
  }

  function goBack(): void {
    const prev = back.value.at(-1)
    back.value = back.value.slice(0, -1)
    place.value = prev ?? { name: 'overview', sessionId: null }
  }

  /** A session went away: leave its view. */
  function forgetSession(id: number): void {
    back.value = back.value.filter((p) => p.sessionId !== id)
    if (place.value.sessionId === id) place.value = back.value.pop() ?? { name: 'overview', sessionId: null }
  }

  return {
    name,
    sessionId,
    projectFilter,
    settingsSection,
    newSessionOpen,
    projectLayout,
    palette,
    canGoBack: computed(() => back.value.length > 0),
    go,
    openSession,
    openSettings,
    openProject,
    openOverview,
    openGrid,
    openFilmstrip,
    goBack,
    forgetSession,
  }
})
