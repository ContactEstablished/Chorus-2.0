/**
 * Dev builds only: a small API on `window.__chorus` for driving the real app from a DevTools
 * Protocol script (as in P0). It goes through the same stores and commands as the UI.
 */
import { commands, unwrap, type AgentKind, type CreateSession, type LaunchRequest, type Prefs, type ProfileSave, type ProjectInput, type ProviderSave } from '../ipc'
import { terminalFor } from '../components/terminals'
import { useSessionsStore } from '../stores/sessions'
import { useAgentsStore } from '../stores/agents'
import { useKeysStore } from '../stores/keys'
import { useProfilesStore } from '../stores/profiles'
import { useProvidersStore } from '../stores/providers'
import { useProjectsStore } from '../stores/projects'
import { usePrefsStore } from '../stores/prefs'
import { useViewStore, type SettingsSection, type ViewName } from '../stores/view'

const plain = <T>(value: T): T => JSON.parse(JSON.stringify(value ?? null)) as T

export function installDevApi(): void {
  const sessions = useSessionsStore()
  const agents = useAgentsStore()
  const keys = useKeysStore()
  const profiles = useProfilesStore()
  const providers = useProvidersStore()
  const projects = useProjectsStore()
  const prefs = usePrefsStore()
  const view = useViewStore()
  /** The session on screen, or the one named. */
  const target = (id?: number) => id ?? view.sessionId ?? sessions.selectedId ?? -1

  const api = {
    list: () => plain(sessions.list),
    create: async (request: Omit<CreateSession, 'options' | 'projectId'> & Partial<Pick<CreateSession, 'options' | 'projectId'>>) => {
      const s = await sessions.create({ ...request, projectId: request.projectId ?? null, options: request.options ?? { effort: null, permission: null, model: null } })
      if (s) view.openSession(s.id)
      return plain(s)
    },
    agents: async (refresh = false) => {
      await agents.load(refresh)
      return plain(agents.list)
    },
    select: (id: number) => view.openSession(id),
    go: (name: ViewName) => view.go(name),
    settings: (section: SettingsSection) => view.openSettings(section),
    view: () => ({ name: view.name, sessionId: view.sessionId, projectFilter: view.projectFilter }),
    write: (id: number, data: string) => unwrap(commands.sessionWrite(id, data)),
    screen: (id?: number) => terminalFor(target(id))?.screenText() ?? '',
    buffer: (id?: number) => terminalFor(target(id))?.bufferText() ?? '',
    restart: (id: number) => sessions.restart(id),
    close: (id: number) => sessions.close(id),
    lock: (id: number, locked: boolean, pin: string | null = null) => sessions.setLocked(id, locked, pin),
    launch: (request: LaunchRequest) => sessions.launch(request).then(plain),
    presets: (agent: AgentKind) => commands.launchPresets(agent).then(plain),
    profiles: async () => (await profiles.load(), plain(profiles.list)),
    profileSave: (input: ProfileSave) => profiles.save(input).then((p) => plain(p ?? profiles.error)),
    rename: (id: number, name: string) => sessions.rename(id, name),
    describe: (id: number, text: string | null) => sessions.setDescription(id, text),
    history: (id: number, limit = 30) => unwrap(commands.sessionHistory(id, limit)).then(plain),
    providers: async () => (await providers.load(), plain(providers.list)),
    providerSave: (input: ProviderSave) => providers.save(input).then((p) => plain(p ?? providers.error)),
    providerTest: (id: number) => providers.test(id).then(() => plain(providers.byId.get(id) ?? providers.error)),
    providerRefresh: (id: number) => providers.refresh(id).then(() => plain(providers.byId.get(id) ?? providers.error)),
    providerCatalog: (id: number) => providers.catalog(id).then(plain),
    providerShortlist: (id: number, models: string[]) => providers.setShortlist(id, models).then(() => plain(providers.byId.get(id))),
    projects: () => plain(projects.list),
    projectSave: (id: number | null, input: ProjectInput) => projects.save(id, input).then(() => plain(projects.list)),
    setProject: (sessionId: number, projectId: number | null) => sessions.setProject(sessionId, projectId).then(plain),
    prefs: () => plain(prefs.prefs),
    setPrefs: (change: Partial<Prefs>) => prefs.update(change).then(() => plain(prefs.prefs)),
    addTestKey: (label: string) => unwrap(commands.devVaultAddTestKey(label)).then(async (k) => (await keys.load(), k)),
    leakCheck: (credentialId: number, id?: number) => unwrap(commands.devLeakCheck(credentialId, terminalFor(target(id))?.screenText() ?? '')),
    error: () => sessions.error,
  }
  ;(window as unknown as { __chorus: typeof api }).__chorus = api
}
