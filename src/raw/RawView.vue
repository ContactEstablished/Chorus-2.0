<script setup lang="ts">
/**
 * P1's bare Raw view: drive the session engine. A list of sessions, the selected one's real
 * terminal, and its only controls, Restart and Close, plus the agent lock. P4 replaces the
 * surroundings with the real app shell; the terminal stays.
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import SessionTerminal from './SessionTerminal.vue'
import NewSessionForm from './NewSessionForm.vue'
import KeysPanel from './KeysPanel.vue'
import ProvidersPanel from './ProvidersPanel.vue'
import { useSessionsStore } from '../stores/sessions'
import { useKeysStore } from '../stores/keys'
import { useProvidersStore } from '../stores/providers'
import {
  commands,
  unwrap,
  type AgentKind,
  type CreateSession,
  type LaunchRequest,
  type ProfileSave,
  type ProviderSave,
  type SessionSummary,
} from '../ipc'
import { useProfilesStore } from '../stores/profiles'
import { AGENT_LABELS, money, observedText, optionsText, versionText } from './labels'
import { useAgentsStore } from '../stores/agents'

const props = defineProps<{ dev: boolean }>()

const sessions = useSessionsStore()
const keys = useKeysStore()
const agents = useAgentsStore()
const providers = useProvidersStore()
const mode = ref<'session' | 'new' | 'keys' | 'providers'>('session')
const busy = ref(false)
const pin = ref('')
const askingPin = ref(false)
const notice = ref<string | null>(null)
const profiles = useProfilesStore()
// Renaming and the note, edited in the header.
const nameDraft = ref<string | null>(null)
const noteDraft = ref<string | null>(null)
const terminal = ref<InstanceType<typeof SessionTerminal> | null>(null)

onMounted(async () => {
  await Promise.all([sessions.init(), keys.load(), agents.load(false), providers.load()])
  if (sessions.list.length === 0) mode.value = 'new'
})
onUnmounted(() => sessions.dispose())

const current = computed(() => (mode.value === 'session' ? sessions.selected : null))

/** The run's own OpenRouter key: its cap, whether it is still live, and what it cost. */
function cappedText(s: SessionSummary): string | null {
  const k = s.cappedKey
  if (!k) return null
  const state = k.live ? (s.live ? 'live' : 'not revoked yet') : 'revoked'
  const cost = k.costUsd === null ? '' : ` · cost ${money(k.costUsd)}`
  return `Run key capped at ${money(k.limitUsd)}, ${state}${cost}${k.note ? ` (${k.note})` : ''}`
}

function statusText(s: SessionSummary): string {
  if (s.live) return s.pid ? `running · pid ${s.pid}` : 'running'
  if (s.status === 'running') return 'starting…'
  return s.lastExitCode === null ? 'exited' : `exited (${s.lastExitCode})`
}

function select(id: number): void {
  sessions.selectedId = id
  mode.value = 'session'
  askingPin.value = false
  notice.value = null
}

async function launch(request: LaunchRequest): Promise<void> {
  busy.value = true
  const out = await sessions.launch(request)
  busy.value = false
  const first = out?.[0]?.session
  if (first) select(first.id)
  const failed = (out ?? []).filter((o) => o.error)
  if (failed.length) {
    notice.value = `Started ${out!.length - failed.length} of ${out!.length}. ${failed.map((o) => `${AGENT_LABELS[o.agent]} couldn't start: ${o.error}`).join(' ')}`
  }
}

async function saveName(s: SessionSummary): Promise<void> {
  if (nameDraft.value !== null && (await sessions.rename(s.id, nameDraft.value))) nameDraft.value = null
}

async function saveNote(s: SessionSummary): Promise<void> {
  if (noteDraft.value !== null && (await sessions.setDescription(s.id, noteDraft.value.trim() || null))) noteDraft.value = null
}

async function restart(s: SessionSummary): Promise<void> {
  busy.value = true
  await sessions.restart(s.id)
  busy.value = false
}

async function close(s: SessionSummary): Promise<void> {
  busy.value = true
  const report = await sessions.close(s.id)
  busy.value = false
  if (report?.spared) {
    notice.value = `Closed. ${report.spared} windowed process${report.spared === 1 ? ' was' : 'es were'} left open (for example a browser it opened).`
  }
  if (sessions.list.length === 0) mode.value = 'new'
}

async function toggleLock(s: SessionSummary): Promise<void> {
  if (s.locked && keys.pinSet && !askingPin.value) {
    askingPin.value = true
    return
  }
  const ok = await sessions.setLocked(s.id, !s.locked, s.locked ? pin.value || null : null)
  pin.value = ''
  if (ok) askingPin.value = false
}

// Dev builds: a small API for driving the real app from a DevTools Protocol script (as in P0).
if (props.dev) {
  const plain = <T,>(value: T): T => JSON.parse(JSON.stringify(value)) as T
  const api = {
    list: () => plain(sessions.list),
    create: async (request: Omit<CreateSession, 'options'> & { options?: CreateSession['options'] }) => {
      const s = await sessions.create({ ...request, options: request.options ?? { effort: null, permission: null, model: null } })
      if (s) select(s.id)
      return plain(sessions.selected)
    },
    agents: async (refresh = false) => {
      await agents.load(refresh)
      return plain(agents.list)
    },
    select,
    write: (id: number, data: string) => unwrap(commands.sessionWrite(id, data)),
    screen: () => terminal.value?.screenText() ?? '',
    buffer: () => terminal.value?.bufferText() ?? '',
    restart: (id: number) => sessions.restart(id),
    close: (id: number) => sessions.close(id),
    lock: (id: number, locked: boolean, pinValue: string | null = null) => sessions.setLocked(id, locked, pinValue),
    launch: (request: LaunchRequest) => sessions.launch(request).then(plain),
    presets: (agent: AgentKind) => commands.launchPresets(agent).then(plain),
    profiles: async () => (await profiles.load(), plain(profiles.list)),
    profileSave: (input: ProfileSave) => profiles.save(input).then((p) => plain(p ?? profiles.error)),
    rename: (id: number, name: string) => sessions.rename(id, name),
    describe: (id: number, text: string | null) => sessions.setDescription(id, text),
    providers: async () => (await providers.load(), plain(providers.list)),
    providerSave: (input: ProviderSave) => providers.save(input).then((p) => plain(p ?? providers.error)),
    providerTest: (id: number) => providers.test(id).then(() => plain(providers.byId.get(id) ?? providers.error)),
    providerRefresh: (id: number) => providers.refresh(id).then(() => plain(providers.byId.get(id) ?? providers.error)),
    providerCatalog: (id: number) => providers.catalog(id).then(plain),
    providerShortlist: (id: number, models: string[]) => providers.setShortlist(id, models).then(() => plain(providers.byId.get(id))),
    addTestKey: (label: string) => unwrap(commands.devVaultAddTestKey(label)).then(async (k) => (await keys.load(), k)),
    leakCheck: (credentialId: number) => unwrap(commands.devLeakCheck(credentialId, terminal.value?.screenText() ?? '')),
    error: () => sessions.error,
  }
  ;(window as unknown as { __chorus: typeof api }).__chorus = api
}
</script>

<template>
  <div class="raw">
    <aside class="list">
      <button class="primary wide" @click="mode = 'new'">New session</button>
      <ul>
        <li v-for="s in sessions.list" :key="s.id">
          <button class="item" :class="{ on: mode === 'session' && s.id === sessions.selectedId }" @click="select(s.id)">
            <span class="dot" :class="s.live ? 'live' : s.status" />
            <span class="name">{{ s.name }}</span>
            <span v-if="s.locked" class="lock" title="Locked">🔒</span>
            <span class="agent">{{ AGENT_LABELS[s.agent] }}</span>
          </button>
        </li>
      </ul>
      <button class="wide quiet" @click="mode = 'keys'">Keys and lock</button>
      <button class="wide quiet" @click="mode = 'providers'">Providers</button>
    </aside>

    <section class="main">
      <p v-if="sessions.error" class="error" role="alert">{{ sessions.error }}</p>

      <NewSessionForm
        v-if="mode === 'new'"
        :keys="keys.keys"
        :busy="busy"
        @launch="launch"
        @cancel="mode = 'session'"
      />
      <KeysPanel v-else-if="mode === 'keys'" />
      <ProvidersPanel v-else-if="mode === 'providers'" />

      <template v-else-if="current">
        <header class="head">
          <div class="title">
            <template v-if="nameDraft !== null">
              <input v-model="nameDraft" maxlength="80" autocomplete="off" @keyup.enter="saveName(current)" @keyup.esc="nameDraft = null" />
              <button @click="saveName(current)">Save</button>
            </template>
            <h2 v-else :title="'Rename'" class="name" @click="nameDraft = current.name">{{ current.name }}</h2>
            <span class="status" :class="current.live ? 'live' : current.status">{{ statusText(current) }}</span>
          </div>
          <div class="controls">
            <template v-if="askingPin">
              <input v-model="pin" type="password" placeholder="PIN" autocomplete="off" @keyup.enter="toggleLock(current)" />
              <button @click="toggleLock(current)">Unlock</button>
              <button class="quiet" @click="askingPin = false">Cancel</button>
            </template>
            <button v-else @click="toggleLock(current)">{{ current.locked ? 'Unlock' : 'Lock' }}</button>
            <button :disabled="busy || current.locked" :title="current.locked ? 'Unlock it first' : ''" @click="restart(current)">
              Restart
            </button>
            <button
              class="danger"
              :disabled="busy || current.locked"
              :title="current.locked ? 'Unlock it first' : ''"
              @click="close(current)"
            >
              Close
            </button>
          </div>
        </header>
        <p v-if="noteDraft !== null" class="description edit">
          <textarea v-model="noteDraft" maxlength="500" rows="2" />
          <button @click="saveNote(current)">Save note</button>
          <button @click="noteDraft = null">Cancel</button>
        </p>
        <p v-else class="description" @click="noteDraft = current.description ?? ''">
          {{ current.description ?? 'Add a note on what this session is for…' }}
        </p>
        <p class="meta">
          {{ AGENT_LABELS[current.agent] }} · <code>{{ current.cwd }}</code>
          <template v-if="current.credentialLabel"> · key {{ current.credentialLabel }} as <code>{{ current.credentialEnv }}</code></template>
        </p>
        <p v-if="current.agent !== 'shell'" class="meta">
          <template v-if="current.providerName">on {{ current.providerName }} · </template>
          <template v-if="optionsText(current)">{{ optionsText(current) }} · </template>
          {{ versionText(current.cliVersion, current.versionFit, agents.byKind.get(current.agent)?.verifiedVersion ?? null) }}
          <template v-if="current.agentSessionId"> · conversation <code :title="current.agentSessionId">{{ current.agentSessionId.slice(0, 8) }}</code>{{ current.resumed ? ' (resumed)' : '' }}</template>
        </p>
        <p v-if="observedText(current)" class="meta" :class="{ differs: current.observedDiffers.length }">
          Newest turn ran {{ observedText(current) }}<template v-if="current.observedDiffers.length">, not what was asked: {{ current.observedDiffers.join('; ') }}</template>
        </p>
        <p v-if="cappedText(current)" class="meta">{{ cappedText(current) }}</p>
        <p v-if="current.note" class="note">{{ current.note }}</p>
        <p v-if="current.scrollbackError" class="note">{{ current.scrollbackError }}</p>
        <p v-if="notice" class="note">{{ notice }}</p>
        <div class="term">
          <SessionTerminal
            :key="current.id"
            ref="terminal"
            :session-id="current.id"
            :live="current.live"
            @error="(m) => (sessions.error = m)"
          />
        </div>
      </template>
      <p v-else class="hint">No session selected.</p>
    </section>
  </div>
</template>

<style scoped>
.raw {
  display: grid;
  grid-template-columns: 240px 1fr;
  height: 100%;
  min-height: 0;
}
.list {
  background: var(--bg-sidebar);
  border-right: 1px solid var(--border);
  padding: 14px 10px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
}
.list ul {
  list-style: none;
  margin: 0;
  padding: 0;
  overflow: auto;
  flex: 1;
}
.wide {
  width: 100%;
}
.item {
  width: 100%;
  display: grid;
  grid-template-columns: 10px 1fr auto;
  grid-template-rows: auto auto;
  column-gap: 8px;
  align-items: center;
  text-align: left;
  background: transparent;
  border: 1px solid transparent;
  padding: 8px 10px;
  border-radius: var(--radius-control);
}
.item.on {
  background: var(--surface);
  border-color: var(--border);
}
.item .name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.item .agent {
  grid-column: 2 / 4;
  color: var(--text-muted);
  font-size: 12px;
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--status-done);
}
.dot.live {
  background: var(--status-ready);
}
.dot.exited {
  background: var(--status-failed);
}
.main {
  padding: 16px 18px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  min-width: 0;
  overflow: auto;
}
.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}
.title {
  display: flex;
  align-items: baseline;
  gap: 12px;
}
h2 {
  font-family: var(--font-heading);
  font-size: 20px;
  margin: 0;
}
.status {
  font-size: 12.5px;
  color: var(--text-muted);
}
.status.live {
  color: var(--status-ready);
}
.status.exited {
  color: var(--status-failed);
}
.controls {
  display: flex;
  gap: 8px;
  align-items: center;
}
.controls input {
  width: 120px;
}
.meta,
.note,
.hint {
  margin: 0;
  color: var(--text-muted);
  font-size: 13px;
}
.note,
.meta.differs {
  color: var(--status-needs);
}
.name {
  cursor: text;
}
.description {
  margin: 0;
  color: var(--text-2);
  font-size: 13.5px;
  cursor: text;
  white-space: pre-wrap;
}
.description.edit {
  display: flex;
  gap: 8px;
  align-items: flex-start;
}
.description.edit textarea {
  flex: 1;
}
.error {
  margin: 0;
  color: var(--status-failed);
  background: rgb(255 61 90 / 0.12);
  padding: 8px 12px;
  border-radius: var(--radius-control);
}
code {
  font-family: var(--font-mono);
  font-size: 12.5px;
  color: var(--text-2);
}
.term {
  flex: 1;
  min-height: 240px;
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
  overflow: hidden;
}
</style>
