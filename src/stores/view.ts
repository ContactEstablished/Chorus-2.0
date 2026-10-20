import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

/** The rail's destinations, plus one session's own view. */
export type ViewName = 'overview' | 'sessions' | 'session' | 'bots' | 'automations' | 'documents' | 'notes' | 'settings'

export type SettingsSection = 'appearance' | 'projects' | 'providers' | 'keys' | 'agents'

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
  /** The Overview's project filter; null is every project. */
  const projectFilter = ref<number | null>(null)
  const settingsSection = ref<SettingsSection>('appearance')
  /** Whether the New session dialog is open. */
  const newSessionOpen = ref(false)

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

  /** Shows the Overview filtered to one project (null: all). */
  function openProject(id: number | null): void {
    projectFilter.value = id
    go('overview')
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
    canGoBack: computed(() => back.value.length > 0),
    go,
    openSession,
    openSettings,
    openProject,
    goBack,
    forgetSession,
  }
})
