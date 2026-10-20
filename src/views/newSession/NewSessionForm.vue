<script setup lang="ts">
/**
 * New session (PLAN P4, following Chorus 1's launch dialog): the agent, how many sessions to start
 * (Solo, Pair, Workbench, Swarm), the folder with Windows' picker and the recent folders, and a
 * worktree suggested when running sessions already work there (D53). Then a suggested name, the
 * project and tags (D52), a note, effort and permissions as button groups, and, folded away, what
 * it runs on: its own sign-in by default, or a provider, a model and a key.
 */
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import {
  commands,
  type AgentKind,
  type CreateSession,
  type CredentialSummary,
  type Effort,
  type FolderCheck,
  type LaunchRequest,
  type Permission,
  type Preset,
  type PresetChoice,
  type ProfileSummary,
  type Workspace,
} from '../../ipc'
import ButtonGroup from '../../components/ButtonGroup.vue'
import TagInput from '../../components/TagInput.vue'
import ProjectPicker from '../../components/ProjectPicker.vue'
import Icon from '../../components/Icon.vue'
import type { Choice } from '../../components/choice'
import { useAgentsStore } from '../../stores/agents'
import { useProfilesStore } from '../../stores/profiles'
import { useProvidersStore } from '../../stores/providers'
import { useProjectsStore } from '../../stores/projects'
import { AGENT_CHIPS } from '../../shell/status'
import { AGENT_LABELS, EFFORT_LABELS, PERMISSION_LABELS, detectionText, money, optionsText } from '../../raw/labels'

const props = defineProps<{ keys: CredentialSummary[]; busy: boolean; initialProject: number | null }>()
const emit = defineEmits<{ launch: [request: LaunchRequest]; cancel: [] }>()

const agents = useAgentsStore()
const providers = useProvidersStore()
const profiles = useProfilesStore()
const projects = useProjectsStore()

// What each agent runs on when no provider or key is chosen.
const SIGN_IN: Record<AgentKind, string> = {
  claude: 'your Claude subscription (its own sign-in)',
  codex: 'your ChatGPT sign-in (codex login)',
  opencode: "opencode's own configuration",
  grok: "Grok's own sign-in",
  shell: '',
}
const AGENT_ORDER: AgentKind[] = ['claude', 'codex', 'opencode', 'grok', 'shell']
const AGENT_KEY = 'chorus.newSession.agent'

function storedAgent(): AgentKind | null {
  try {
    const a = localStorage.getItem(AGENT_KEY)
    return AGENT_ORDER.includes(a as AgentKind) ? (a as AgentKind) : null
  } catch {
    return null
  }
}

const agent = ref<AgentKind>(storedAgent() ?? 'claude')
const preset = ref<Preset>('solo')
const swarmCount = ref(3)
const cwd = ref('')
const recent = ref<string[]>([])
const check = ref<FolderCheck | null>(null)
const workspace = ref<Workspace>('here')
// Once the user picks a workspace, the folder check stops choosing for them.
const workspaceChosen = ref(false)
const name = ref('')
const offeredNames = ref<string[]>([])
const projectId = ref<number | null>(props.initialProject)
const tags = ref<string[]>([])
const knownTags = ref<string[]>([])
const description = ref('')
// null = the agent's declared default.
const effort = ref<Effort | null>(null)
const permission = ref<Permission | null>(null)
const model = ref('')
// null = the agent's own sign-in, or a key given below.
const providerId = ref<number | null>(null)
const keyId = ref<number | null>(null)
const envVar = ref('')
const runsOnOpen = ref(false)
const presetChoices = ref<PresetChoice[]>([])

const info = computed(() => agents.byKind.get(agent.value) ?? null)

onMounted(async () => {
  void agents.load(true)
  void providers.load()
  void profiles.load()
  void loadPresets()
  void suggestName()
  commands.tagsKnown().then((r) => {
    if (r.status === 'ok') knownTags.value = r.data.map((t) => t.tag)
  })
  recent.value = await commands.foldersRecent().catch(() => [])
  const projectFolder = props.initialProject === null ? null : (projects.byId.get(props.initialProject)?.folder ?? null)
  if (!cwd.value) cwd.value = projectFolder ?? recent.value[0] ?? ''
})

// Agents, as buttons. One Chorus couldn't find says why and can't be chosen.
const agentChoices = computed<Choice<AgentKind>[]>(() =>
  AGENT_ORDER.map((k) => {
    const a = agents.byKind.get(k)
    const missing = a?.detection && !a.detection.found
    return { value: k, label: a?.displayName ?? AGENT_LABELS[k], disabled: !!missing, title: missing ? (a?.detection?.problem ?? 'Not found') : undefined }
  }),
)
watch(
  () => agents.list.length,
  () => {
    // The remembered agent may have gone: fall back to the first one found.
    if (agentChoices.value.find((c) => c.value === agent.value)?.disabled) {
      agent.value = agentChoices.value.find((c) => !c.disabled)?.value ?? 'shell'
    }
  },
)
const agentWarning = computed(() => {
  const a = info.value
  if (!a?.detection || a.kind === 'shell') return null
  return !a.detection.found || a.detection.fit !== 'verified' ? detectionText(a) : null
})

// A choice made for one agent may not exist for the next.
watch(agent, () => {
  effort.value = null
  permission.value = null
  model.value = ''
  providerId.value = null
  void loadPresets()
})

async function loadPresets(): Promise<void> {
  presetChoices.value = await commands.launchPresets(agent.value).catch(() => [])
  if (presetChoices.value.find((p) => p.preset === preset.value)?.reason) preset.value = 'solo'
}
const chosenPreset = computed(() => presetChoices.value.find((p) => p.preset === preset.value) ?? null)
const presetOptions = computed<Choice<Preset>[]>(() =>
  presetChoices.value.map((p) => ({ value: p.preset, label: p.label, disabled: p.reason !== null, title: p.reason ?? p.blurb })),
)
const countOptions: Choice<number>[] = [2, 3, 4, 5, 6].map((n) => ({ value: n, label: String(n) }))
const sessionCount = computed(() => (preset.value === 'swarm' ? swarmCount.value : (chosenPreset.value?.slots.length ?? 1)))
const presetText = computed(() => {
  const p = chosenPreset.value
  if (!p) return ''
  if (p.preset === 'pair') return `${AGENT_LABELS[agent.value]} builds; ${AGENT_LABELS[p.slots[1]?.agent ?? 'claude']} reviews. ${p.note ?? ''}`
  return [p.blurb + '.', p.note].filter(Boolean).join(' ')
})

// The folder: checked as it's typed or picked.
let checkTimer: ReturnType<typeof setTimeout> | undefined
let checkSeq = 0
watch(cwd, (folder) => {
  clearTimeout(checkTimer)
  if (!folder.trim()) {
    check.value = null
    return
  }
  checkTimer = setTimeout(async () => {
    const seq = ++checkSeq
    const r = await commands.folderCheck(folder.trim()).catch(() => null)
    if (seq === checkSeq && r?.status === 'ok') check.value = r.data
  }, 250)
})
const git = computed(() => check.value?.git ?? null)
const canWorktree = computed(() => !!git.value && !git.value.worktreeProblem)
watch(check, (c) => {
  if (!canWorktree.value) workspace.value = 'here'
  else if (!workspaceChosen.value) workspace.value = c?.sharing.length ? 'new-worktree' : 'here'
})
const workspaceOptions = computed<Choice<Workspace>[]>(() => [
  { value: 'here', label: 'This folder' },
  { value: 'new-worktree', label: 'New worktree', disabled: !canWorktree.value, title: git.value?.worktreeProblem ?? undefined },
])
function chooseWorkspace(w: Workspace): void {
  workspace.value = w
  workspaceChosen.value = true
}
const branchText = computed(() => (git.value ? (git.value.branch ?? 'a detached HEAD') : ''))
const sharingText = computed(() => {
  const s = check.value?.sharing ?? []
  if (!s.length) return ''
  const who = s.map((x) => `${x.name} (${AGENT_LABELS[x.agent]})`)
  const list = who.length > 2 ? `${who.slice(0, 2).join(', ')} and ${who.length - 2} more` : who.join(' and ')
  return `${list} ${s.length === 1 ? 'is' : 'are'} already working on ${branchText.value} here.`
})
const autoProject = computed(() => (check.value?.projectId == null ? null : (projects.byId.get(check.value.projectId) ?? null)))
const recentChips = computed(() => recent.value.filter((f) => f.toLowerCase() !== cwd.value.trim().toLowerCase()).slice(0, 5))
function leaf(path: string): string {
  return path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || path
}
async function browse(): Promise<void> {
  const picked = await commands.folderPick(cwd.value.trim() || null).catch(() => null)
  if (picked) cwd.value = picked
}

// The name: suggested from Chorus 1's pool; another on request.
async function suggestName(): Promise<void> {
  const next = await commands.sessionSuggestName(offeredNames.value).catch(() => '')
  if (!next) return
  offeredNames.value = [...offeredNames.value, next]
  name.value = next
}

// Effort and permissions: the agent's default shows as chosen; with no default, clicking the
// chosen one clears it so the CLI decides.
const effortShown = computed({
  get: () => effort.value ?? info.value?.defaultEffort ?? null,
  set: (v: Effort | null) => (effort.value = v),
})
const permissionShown = computed({
  get: () => permission.value ?? info.value?.defaultPermission ?? null,
  set: (v: Permission | null) => (permission.value = v),
})
const effortOptions = computed<Choice<Effort | null>[]>(() =>
  (info.value?.efforts ?? []).map((e) => ({ value: e.effort, label: EFFORT_LABELS[e.effort], title: e.args })),
)
const permissionOptions = computed<Choice<Permission | null>[]>(() =>
  (info.value?.permissions ?? []).map((p) => ({ value: p.permission, label: PERMISSION_LABELS[p.permission], title: p.args })),
)
const carrier = computed(() => (info.value?.optionsInConfig ? 'server config: ' : ''))
const effortArgs = computed(() => info.value?.efforts.find((e) => e.effort === effortShown.value)?.args ?? '')
const permissionArgs = computed(() => info.value?.permissions.find((p) => p.permission === permissionShown.value)?.args ?? '')
// opencode applies an effort only to a model named beside it (LESSONS L5).
const effortWithoutModel = computed(() => !!info.value?.effortNeedsModel && effortShown.value !== null && model.value.trim() === '')

// What it runs on.
const routes = computed(() =>
  providers.list
    .filter((p) => p.kind !== 'jev')
    .map((p) => ({ provider: p, reason: p.agents.find((a) => a.agent === agent.value)?.reason ?? null })),
)
const provider = computed(() => (providerId.value === null ? null : (providers.byId.get(providerId.value) ?? null)))
const keyNote = computed(() => {
  const p = provider.value
  if (!p) return ''
  if (p.kind === 'openrouter' && p.managementCredentialId !== null) {
    return `Each run gets its own OpenRouter key, capped at ${money(p.capUsd ?? p.defaultCapUsd)}, and revoked when it ends.`
  }
  return `Runs use the key ${p.credentialLabel ?? '(none set)'}, without a cap.`
})
const runsOnText = computed(() => {
  const key = keyId.value === null ? '' : `, with the key ${props.keys.find((k) => k.id === keyId.value)?.label ?? '?'}`
  const m = model.value.trim()
  if (provider.value) return `${provider.value.name}${m ? ` · ${m}` : ''}${key}`
  return `${SIGN_IN[agent.value]}${m ? ` · ${m}` : ''}${key}`
})
// opencode names its model up front; the others only when you open the section.
const modelUpFront = computed(() => !!info.value?.optionsInConfig)

// Profiles: fill the form from one, or save the form as one. The agent goes first, because
// changing it clears the other choices.
const profileName = ref('')
const savedNote = ref('')
async function applyProfile(p: ProfileSummary): Promise<void> {
  agent.value = p.agent
  await nextTick()
  effort.value = p.options.effort
  permission.value = p.options.permission
  model.value = p.options.model ?? ''
  providerId.value = p.providerId
  keyId.value = p.credential?.id ?? null
  envVar.value = p.credential?.envVar ?? ''
  profileName.value = p.name
  if (p.providerId !== null || p.credential) runsOnOpen.value = true
  savedNote.value = p.providerName === '(deleted provider)' ? 'Its provider was deleted: choose another.' : `Filled from ${p.name}.`
}
async function saveProfile(): Promise<void> {
  const existing = profiles.list.find((p) => p.name === profileName.value.trim())
  const saved = await profiles.save({
    id: existing?.id ?? null,
    name: profileName.value,
    agent: agent.value,
    options: { effort: effort.value, permission: permission.value, model: model.value.trim() || null },
    providerId: providerId.value,
    credential: keyId.value === null ? null : { id: keyId.value, envVar: envVar.value.trim() },
  })
  savedNote.value = saved ? `Saved as ${saved.name}.` : ''
}

const blocker = computed(() => {
  if (!cwd.value.trim()) return 'Choose a folder.'
  if (check.value?.problem) return check.value.problem
  if (chosenPreset.value?.reason) return chosenPreset.value.reason
  if (preset.value === 'swarm' && !canWorktree.value) return git.value?.worktreeProblem ?? 'A Swarm needs a git repository: each agent works in a worktree of its own.'
  if (keyId.value !== null && !envVar.value.trim()) return 'Name the variable the key goes in.'
  if (effortWithoutModel.value) return `${info.value?.displayName} applies an effort only to a model named beside it.`
  if (providerId.value !== null && !model.value.trim()) return `Choose a ${provider.value?.name ?? ''} model.`
  return null
})

function submit(): void {
  if (blocker.value || props.busy) return
  try {
    localStorage.setItem(AGENT_KEY, agent.value)
  } catch {
    // Remembering the agent is a convenience.
  }
  const session: CreateSession = {
    agent: agent.value,
    cwd: cwd.value.trim(),
    name: name.value.trim() || null,
    credential: keyId.value === null ? null : { id: keyId.value, envVar: envVar.value.trim() },
    cols: 120,
    rows: 30,
    options: { effort: effort.value, permission: permission.value, model: model.value.trim() || null },
    providerId: providerId.value,
    description: description.value.trim() || null,
    projectId: projectId.value,
    tags: tags.value,
    workspace: preset.value === 'swarm' ? 'new-worktree' : workspace.value,
  }
  emit('launch', { preset: preset.value, session, count: preset.value === 'swarm' ? swarmCount.value : null })
}

// Ctrl+Enter starts from anywhere in the form.
function onKey(e: KeyboardEvent): void {
  if (e.key === 'Enter' && e.ctrlKey) {
    e.preventDefault()
    submit()
  }
}
</script>

<template>
  <form class="form" @submit.prevent="submit" @keydown="onKey">
    <h2>New session</h2>

    <div v-if="profiles.list.length" class="chips">
      <span class="label">Profiles</span>
      <span v-for="p in profiles.list" :key="p.id" class="chip">
        <button type="button" :title="optionsText(p) || 'defaults'" @click="applyProfile(p)">
          {{ p.name }} <em>{{ AGENT_LABELS[p.agent] }}{{ p.providerName ? ` · ${p.providerName}` : '' }}</em>
        </button>
        <button type="button" class="x" :title="`Delete ${p.name}`" @click="profiles.remove(p.id)">×</button>
      </span>
    </div>
    <p v-if="savedNote" class="hint">{{ savedNote }}</p>

    <div class="field">
      <span class="label">Agent</span>
      <ButtonGroup v-model="agent" :options="agentChoices" label="Agent">
        <template #option="{ choice }">
          <span class="agent-chip" :class="`agent-${choice.value}`">{{ AGENT_CHIPS[choice.value] }}</span>{{ choice.label }}
        </template>
      </ButtonGroup>
      <p v-if="agentWarning" class="hint warn">{{ agentWarning }}</p>
      <p v-else-if="agents.error" class="hint warn">{{ agents.error }}</p>
    </div>

    <div class="field">
      <span class="label">Start</span>
      <ButtonGroup v-model="preset" :options="presetOptions" label="Start" />
      <div v-if="preset === 'swarm'" class="count">
        <span class="label">How many</span>
        <ButtonGroup v-model="swarmCount" :options="countOptions" label="How many agents" />
      </div>
      <p class="hint">{{ presetText }}</p>
    </div>

    <div class="field">
      <span class="label">Folder</span>
      <div class="row">
        <input v-model="cwd" class="mono" placeholder="C:\Projects\app" spellcheck="false" autocomplete="off" aria-label="Folder" />
        <button type="button" @click="browse"><Icon name="folder" :size="15" /> Browse…</button>
      </div>
      <div v-if="recentChips.length" class="chips">
        <span class="label small">Recent</span>
        <button v-for="f in recentChips" :key="f" type="button" class="folder-chip" :title="f" @click="cwd = f">{{ leaf(f) }}</button>
      </div>
      <p v-if="check?.problem" class="hint bad">{{ check.problem }}</p>
      <p v-else-if="git" class="hint">
        <Icon name="branch" :size="13" /> {{ git.repo }} on <strong>{{ branchText }}</strong>{{ git.inWorktree ? ', in one of its worktrees' : '' }}
      </p>
      <p v-else-if="check?.gitNote" class="hint warn">{{ check.gitNote }}</p>
      <p v-else-if="check" class="hint">Not a git repository: sessions work in the folder itself.</p>
    </div>

    <div v-if="git && preset !== 'swarm'" class="field">
      <span class="label">Work in</span>
      <ButtonGroup :model-value="workspace" :options="workspaceOptions" label="Work in" @update:model-value="chooseWorkspace" />
      <p v-if="sharingText" class="hint" :class="{ warn: workspace === 'here' }">
        {{ sharingText }}
        <template v-if="workspace === 'here'">This session would change the same files on the same branch.</template>
        <template v-else>A new worktree gives this one its own branch, {{ git.newBranch }}, made from {{ branchText }}.</template>
      </p>
      <p v-else-if="workspace === 'new-worktree'" class="hint">
        Its own branch, {{ git.newBranch }}, made from {{ branchText }}, in a folder beside the repository.
        <template v-if="sessionCount > 1">All {{ sessionCount }} sessions work in it.</template>
      </p>
    </div>
    <p v-else-if="preset === 'swarm' && git" class="hint">Each agent gets a worktree and branch of its own ({{ git.newBranch }}), made from {{ branchText }}.</p>

    <div class="pair">
      <div class="field">
        <span class="label">Name</span>
        <div class="row">
          <input v-model="name" maxlength="80" autocomplete="off" aria-label="Name" placeholder="unnamed" />
          <button type="button" class="quiet icon-button" title="Suggest another name" aria-label="Suggest another name" @click="suggestName">
            <Icon name="shuffle" :size="15" />
          </button>
        </div>
      </div>
      <div class="field">
        <span class="label">Project</span>
        <ProjectPicker v-model="projectId" :auto="autoProject" :new-folder="git?.topLevel ?? (check && !check.problem ? cwd.trim() : null)" />
      </div>
    </div>

    <div class="field">
      <span class="label">Tags</span>
      <TagInput v-model="tags" :known="knownTags" />
    </div>

    <div class="field">
      <span class="label">Note <em>(optional: what this session is for)</em></span>
      <textarea v-model="description" maxlength="500" rows="2" aria-label="Note" />
    </div>

    <template v-if="info && info.kind !== 'shell'">
      <div v-if="effortOptions.length" class="field">
        <span class="label">Effort</span>
        <ButtonGroup v-model="effortShown" :options="effortOptions" label="Effort" :clearable="info.defaultEffort === null" />
        <p class="hint">
          <code v-if="effortArgs">{{ carrier }}{{ effortArgs }}</code>
          <template v-else-if="info.defaultEffort === null">None chosen: {{ info.displayName }}'s own setting applies.</template>
        </p>
      </div>
      <p v-else class="hint">{{ info.displayName }} has no effort setting Chorus can pass at launch yet.</p>

      <div v-if="permissionOptions.length" class="field">
        <span class="label">Permissions</span>
        <ButtonGroup v-model="permissionShown" :options="permissionOptions" label="Permissions" :clearable="info.defaultPermission === null" />
        <p class="hint">
          <code v-if="permissionArgs">{{ carrier }}{{ permissionArgs }}</code>
          <template v-else-if="info.defaultPermission === null">None chosen: {{ info.displayName }}'s own setting applies.</template>
        </p>
      </div>
      <p v-else class="hint">{{ info.displayName }} has no permission mode Chorus can pass at launch yet.</p>

      <div v-if="modelUpFront" class="field">
        <span class="label">Model <em>{{ provider ? `(required: a ${provider.name} model id)` : '(optional)' }}</em></span>
        <input v-model="model" class="mono" maxlength="200" spellcheck="false" autocomplete="off" list="provider-shortlist" placeholder="provider/model, for example opencode/big-pickle" />
      </div>

      <div class="runs-on">
        <button type="button" class="quiet disclosure" :aria-expanded="runsOnOpen" @click="runsOnOpen = !runsOnOpen">
          <span class="caret" :class="{ open: runsOnOpen }">›</span>
          Runs on {{ runsOnText }}
        </button>
        <div v-if="runsOnOpen" class="advanced">
          <label v-if="!modelUpFront" class="field">
            <span class="label">Model <em>{{ provider ? `(required: a ${provider.name} model id)` : "(optional: the CLI's default otherwise)" }}</em></span>
            <input v-model="model" class="mono" maxlength="200" spellcheck="false" autocomplete="off" list="provider-shortlist" :placeholder="provider?.shortlist[0] ?? ''" />
          </label>
          <label v-if="routes.length" class="field">
            <span class="label">Provider</span>
            <select v-model="providerId">
              <option :value="null">None: {{ SIGN_IN[agent] }}</option>
              <option v-for="r in routes" :key="r.provider.id" :value="r.provider.id" :disabled="r.reason !== null">
                {{ r.provider.name }}{{ r.reason ? ` (not for ${info.displayName}: ${r.reason})` : '' }}
              </option>
            </select>
            <span v-if="keyNote" class="hint">{{ keyNote }}</span>
          </label>
          <label class="field">
            <span class="label">Key <em>(optional)</em></span>
            <select v-model="keyId">
              <option :value="null">None</option>
              <option v-for="k in keys" :key="k.id" :value="k.id">{{ k.label }} · {{ k.fingerprint }}</option>
            </select>
          </label>
          <label v-if="keyId !== null" class="field">
            <span class="label">As variable</span>
            <input v-model="envVar" class="mono" placeholder="OPENROUTER_API_KEY" spellcheck="false" autocomplete="off" />
            <span class="hint">The key reaches the process only as this environment variable, and is redacted from its output.</span>
          </label>
          <div class="row">
            <input v-model="profileName" placeholder="Profile name" maxlength="80" autocomplete="off" aria-label="Profile name" />
            <button type="button" :disabled="!profileName.trim()" @click="saveProfile">
              {{ profiles.list.some((p) => p.name === profileName.trim()) ? 'Update profile' : 'Save as profile' }}
            </button>
          </div>
          <p v-if="profiles.error" class="hint warn">{{ profiles.error }}</p>
        </div>
      </div>
      <datalist id="provider-shortlist">
        <option v-for="m in provider?.shortlist ?? []" :key="m" :value="m" />
      </datalist>
    </template>

    <div class="actions">
      <span v-if="blocker && cwd.trim()" class="hint warn why">{{ blocker }}</span>
      <button type="button" @click="emit('cancel')">Cancel</button>
      <button type="submit" class="primary" :disabled="!!blocker || busy" title="Start (Ctrl+Enter)">
        {{ busy ? 'Starting…' : sessionCount > 1 ? `Start ${sessionCount} sessions` : 'Start' }}
      </button>
    </div>
  </form>
</template>

<style scoped>
.form {
  display: grid;
  gap: 16px;
}
h2 {
  font-family: var(--font-heading);
  font-size: 20px;
  margin: 0;
}
.field {
  display: grid;
  gap: 6px;
  min-width: 0;
}
.label {
  color: var(--text-2);
  font-size: 13px;
  font-weight: 600;
}
.label.small {
  font-weight: 500;
  font-size: 12px;
  color: var(--text-muted);
}
em {
  color: var(--text-muted);
  font-style: normal;
  font-weight: 400;
}
.hint {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0 5px;
  margin: 0;
  color: var(--text-muted);
  font-size: 12.5px;
}
.hint.warn {
  color: var(--status-needs);
}
.hint.bad {
  color: var(--status-failed);
}
.hint code {
  font-size: 11.5px;
}
.row {
  display: flex;
  gap: 8px;
}
.row input {
  flex: 1;
}
.row > button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.mono {
  font-family: var(--font-mono);
  font-size: 12.5px;
}
.pair {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}
.icon-button {
  padding: 6px 9px;
}
.count {
  display: flex;
  align-items: center;
  gap: 10px;
}
.count :deep(.group) {
  flex: 0 1 260px;
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: center;
}
.folder-chip {
  padding: 2px 10px;
  border-radius: var(--radius-pill);
  font-size: 12px;
}
.chip {
  display: inline-flex;
  border: 1px solid var(--border);
  border-radius: var(--radius-pill);
  overflow: hidden;
}
.chip button {
  border: 0;
  border-radius: 0;
  background: transparent;
  padding: 3px 10px;
  font-size: 12.5px;
}
.chip .x {
  padding: 3px 8px;
  color: var(--text-muted);
}
.runs-on {
  border-top: 1px solid var(--border);
  padding-top: 10px;
}
.disclosure {
  display: flex;
  align-items: baseline;
  gap: 8px;
  padding: 4px 0;
  text-align: left;
  font-size: 13px;
}
.caret {
  display: inline-block;
  transition: transform 120ms ease;
}
.caret.open {
  transform: rotate(90deg);
}
.advanced {
  display: grid;
  gap: 12px;
  padding: 10px 0 0 18px;
}
.actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
  padding-top: 4px;
}
.why {
  margin-right: auto;
}
@media (max-width: 560px) {
  .pair {
    grid-template-columns: 1fr;
  }
}
</style>
