<script setup lang="ts">
/**
 * New session, in the two-pane layout of Matthew's design (1c, D56). The form: the agent as logo
 * tiles, the folder with Windows' picker and the recent folders, a worktree suggested when running
 * sessions already work there (D53), a suggested name, the project and tags (D52) with a note, and
 * effort and permissions; folded away, what it runs on (its own sign-in by default; a provider, a
 * model and a key). The summary: the mode (Solo, Pair, Workbench, Swarm), what will start, the exact
 * arguments, and Start.
 */
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
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
  type RoutingLaunchChoice,
  type Workspace,
} from '../../ipc'
import RoutingTiers from './RoutingTiers.vue'
import ButtonGroup from '../../components/ButtonGroup.vue'
import TagInput from '../../components/TagInput.vue'
import ProjectPicker from '../../components/ProjectPicker.vue'
import Icon from '../../components/Icon.vue'
import type { Choice } from '../../components/choice'
import { useAgentsStore } from '../../stores/agents'
import { useProfilesStore } from '../../stores/profiles'
import { useProvidersStore } from '../../stores/providers'
import { useProjectsStore } from '../../stores/projects'
import { AGENT_LABELS, EFFORT_LABELS, PERMISSION_LABELS, money, optionsText } from '../../raw/labels'
import claudeLogo from '../../assets/agents/claude.png'
import codexLogo from '../../assets/agents/codex.png'
import opencodeLogo from '../../assets/agents/opencode.png'
import grokLogo from '../../assets/agents/grok.png'
import geminiLogo from '../../assets/agents/gemini.png'
import shellLogo from '../../assets/agents/shell.png'

const props = defineProps<{ keys: CredentialSummary[]; busy: boolean; initialProject: number | null }>()
const emit = defineEmits<{ launch: [request: LaunchRequest]; cancel: [] }>()

const agents = useAgentsStore()
const providers = useProvidersStore()
const profiles = useProfilesStore()
const projects = useProjectsStore()

// Drawn in the theme's colours: each logo is a mask.
const LOGOS: Record<AgentKind, string> = { claude: claudeLogo, codex: codexLogo, opencode: opencodeLogo, grok: grokLogo, gemini: geminiLogo, shell: shellLogo }
// The program each agent runs, for the summary's command line.
const PROGRAMS: Record<AgentKind, string> = { claude: 'claude', codex: 'codex', opencode: 'opencode', grok: 'grok', gemini: 'gemini', shell: 'cmd' }
// What each agent runs on when no provider or key is chosen.
const SIGN_IN: Record<AgentKind, string> = {
  claude: 'your Claude subscription',
  codex: 'your ChatGPT sign-in',
  opencode: "opencode's own configuration",
  grok: "Grok's own sign-in",
  gemini: 'your Google sign-in or a Gemini key',
  shell: 'no key: a plain terminal',
}
const SHORT_MODE: Record<Preset, string> = { solo: 'Solo', pair: 'Pair', workbench: 'Bench', swarm: 'Swarm' }
const AGENT_ORDER: AgentKind[] = ['claude', 'codex', 'opencode', 'gemini', 'grok', 'shell']
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
const routing = ref<RoutingLaunchChoice | null>(null)
const runsOnOpen = ref(false)
const runsOnEl = ref<HTMLElement | null>(null)
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

// Agents, as logo tiles. One Chorus couldn't find says why and can't be chosen.
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
// The detected version, and whether its options were checked against it.
const version = computed(() => {
  const d = info.value?.detection
  if (!d || agent.value === 'shell') return null
  if (!d.found) return { text: d.problem ?? 'not found', warn: true }
  const v = d.version ?? 'version unknown'
  return d.fit === 'verified' ? { text: v, warn: false } : { text: `${v} · untested`, warn: true, title: `Options were checked against ${info.value?.verifiedVersion ?? '?'}` }
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
  presetChoices.value.map((p) => ({ value: p.preset, label: SHORT_MODE[p.preset], disabled: p.reason !== null, title: p.reason ?? `${p.label}: ${p.blurb}` })),
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
const chosenProject = computed(() => (projectId.value === null ? null : (projects.byId.get(projectId.value) ?? null)))
const recentChips = computed(() => recent.value.filter((f) => f.toLowerCase() !== cwd.value.trim().toLowerCase()).slice(0, 5))
function leaf(path: string): string {
  return path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || path
}
// A long path shows its end (the folder's own name) unless it's being edited.
const folderEl = ref<HTMLInputElement | null>(null)
function showFolderEnd(): void {
  const el = folderEl.value
  if (el && document.activeElement !== el) el.scrollLeft = el.scrollWidth
}
watch(cwd, () => void nextTick(showFolderEnd))
// Again once the monospace font has loaded and the dialog has finished rising: both change widths.
onMounted(() => {
  void document.fonts?.ready.then(showFolderEnd)
  setTimeout(showFolderEnd, 300)
})

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
// The value an option sets, shown beside the chosen one (`xhigh` from `--effort xhigh`).
function lastWord(args: string): string {
  return args.trim().split(/\s+/).pop()?.replace(/^.*=/, '').replace(/"/g, '') ?? ''
}
const effortOptions = computed<Choice<Effort | null>[]>(() =>
  (info.value?.efforts ?? []).map((e) => ({ value: e.effort, label: EFFORT_LABELS[e.effort], title: e.args, hint: lastWord(e.args) })),
)
const permissionOptions = computed<Choice<Permission | null>[]>(() =>
  (info.value?.permissions ?? []).map((p) => ({ value: p.permission, label: PERMISSION_LABELS[p.permission], title: p.args, hint: lastWord(p.args) })),
)
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
const keyLabel = computed(() => (keyId.value === null ? null : (props.keys.find((k) => k.id === keyId.value)?.label ?? '?')))
const runsOnText = computed(() => {
  const key = keyLabel.value ? ` with the key ${keyLabel.value}` : ''
  return provider.value ? `${provider.value.name}${key}` : `${SIGN_IN[agent.value]}${key}`
})
// opencode names its model and provider up front; the others only when what it runs on is opened.
const modelUpFront = computed(() => !!info.value?.optionsInConfig)
// Routing tiers (D57): opencode on an OpenRouter provider, with a model named.
const routingApplies = computed(
  () => agent.value === 'opencode' && provider.value?.kind === 'openrouter' && /^[a-z0-9][\w.@~-]*\/[\w.@~:-]+$/i.test(model.value.trim()),
)
const ROUTING_LABELS: Record<RoutingLaunchChoice, string> = { budget: 'Budget', balanced: 'Balanced', fast: 'Fast', nitro: 'Nitro', default: 'OpenRouter default' }
function openRunsOn(): void {
  runsOnOpen.value = true
  void nextTick(() => runsOnEl.value?.scrollIntoView({ block: 'nearest', behavior: 'smooth' }))
}

// The exact arguments the options add, from the back end (the same functions the launch uses).
const args = ref<string[]>([])
const argsProblem = ref<string | null>(null)
let argsSeq = 0
watch(
  [agent, effort, permission, model],
  async () => {
    const seq = ++argsSeq
    const r = await commands
      .launchArgs(agent.value, { effort: effort.value, permission: permission.value, model: model.value.trim() || null })
      .catch(() => null)
    if (seq !== argsSeq || !r) return
    args.value = r.status === 'ok' ? r.data : []
    argsProblem.value = r.status === 'ok' ? null : r.error
  },
  { immediate: true },
)

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
  if (argsProblem.value) return argsProblem.value
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
    routing: routingApplies.value ? routing.value : null,
  }
  emit('launch', { preset: preset.value, session, count: preset.value === 'swarm' ? swarmCount.value : null })
}

// Ctrl+Enter starts from anywhere while the dialog is open, focus or not.
function onKey(e: KeyboardEvent): void {
  if (e.key === 'Enter' && e.ctrlKey) {
    e.preventDefault()
    submit()
  }
}
onMounted(() => window.addEventListener('keydown', onKey))
onUnmounted(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <form class="box" @submit.prevent="submit">
    <div class="form scroll">
      <h2>New session</h2>

      <div v-if="profiles.list.length" class="chips">
        <span class="small">Profiles</span>
        <span v-for="p in profiles.list" :key="p.id" class="chip">
          <button type="button" :title="optionsText(p) || 'defaults'" @click="applyProfile(p)">
            {{ p.name }} <em>{{ AGENT_LABELS[p.agent] }}{{ p.providerName ? ` · ${p.providerName}` : '' }}</em>
          </button>
          <button type="button" class="x" :title="`Delete ${p.name}`" @click="profiles.remove(p.id)">×</button>
        </span>
      </div>
      <p v-if="savedNote" class="hint">{{ savedNote }}</p>

      <section class="field">
        <span class="label">Agent</span>
        <ButtonGroup v-model="agent" :options="agentChoices" label="Agent" variant="tiles">
          <template #option="{ choice }">
            <span class="logo" :style="{ '--logo': `url(${LOGOS[choice.value]})` }" />
            <span class="agent-name">{{ choice.label }}</span>
          </template>
        </ButtonGroup>
        <p v-if="agents.error" class="hint warn">{{ agents.error }}</p>
      </section>

      <section class="field">
        <span class="label">Folder</span>
        <div class="boxed folder">
          <input ref="folderEl" v-model="cwd" class="mono" placeholder="C:\Projects\app" spellcheck="false" autocomplete="off" aria-label="Folder" @blur="showFolderEnd" />
          <button type="button" class="inner" @click="browse"><Icon name="folder" :size="14" /> Browse…</button>
        </div>
        <div v-if="recentChips.length" class="chips">
          <span class="small">Recent</span>
          <button v-for="f in recentChips" :key="f" type="button" class="folder-chip" :title="f" @click="cwd = f">{{ leaf(f) }}</button>
        </div>
        <p v-if="check?.problem" class="hint bad">{{ check.problem }}</p>
        <p v-else-if="check?.gitNote" class="hint warn">{{ check.gitNote }}</p>
        <p v-else-if="check && !git" class="hint">Not a git repository: sessions work in the folder itself.</p>
        <template v-if="git && preset !== 'swarm'">
          <ButtonGroup :model-value="workspace" :options="workspaceOptions" label="Work in" variant="chips" @update:model-value="chooseWorkspace" />
          <p v-if="sharingText" class="hint" :class="{ warn: workspace === 'here' }">
            {{ sharingText }}
            <template v-if="workspace === 'here'">This session would change the same files on the same branch.</template>
            <template v-else>A new worktree gives this one its own branch, {{ git.newBranch }}, made from {{ branchText }}.</template>
          </p>
          <p v-else-if="workspace === 'new-worktree'" class="hint">
            Its own branch, {{ git.newBranch }}, made from {{ branchText }}, in a folder beside the repository.
            <template v-if="sessionCount > 1">All {{ sessionCount }} sessions work in it.</template>
          </p>
        </template>
        <p v-else-if="git" class="hint">Each agent gets a worktree and branch of its own ({{ git.newBranch }}), made from {{ branchText }}.</p>
      </section>

      <section class="pair">
        <div class="field">
          <span class="label">Name</span>
          <div class="boxed">
            <input v-model="name" maxlength="80" autocomplete="off" aria-label="Name" placeholder="unnamed" />
            <button type="button" class="quiet inner icon" title="Suggest another name" aria-label="Suggest another name" @click="suggestName">
              <Icon name="shuffle" :size="15" />
            </button>
          </div>
        </div>
        <div class="field">
          <span class="label">Project</span>
          <ProjectPicker v-model="projectId" :auto="autoProject" :new-folder="git?.topLevel ?? (check && !check.problem ? cwd.trim() : null)" />
        </div>
      </section>

      <section class="field">
        <span class="label">Tags <em>&amp; note</em></span>
        <div class="tags-note">
          <TagInput v-model="tags" :known="knownTags" />
          <textarea v-model="description" maxlength="500" rows="2" placeholder="What is this session for?" aria-label="Note" />
        </div>
      </section>

      <template v-if="info && info.kind !== 'shell'">
        <section class="pair">
          <div class="field">
            <span class="label">Effort</span>
            <ButtonGroup v-if="effortOptions.length" v-model="effortShown" :options="effortOptions" label="Effort" variant="radios" :clearable="info.defaultEffort === null" />
            <p v-else class="hint">{{ info.displayName }} has no effort setting Chorus can pass at launch yet.</p>
            <p v-if="effortOptions.length && effortShown === null" class="hint">None chosen: {{ info.displayName }}'s own setting applies.</p>
          </div>
          <div class="field">
            <span class="label">Permissions</span>
            <ButtonGroup v-if="permissionOptions.length" v-model="permissionShown" :options="permissionOptions" label="Permissions" variant="radios" :clearable="info.defaultPermission === null" />
            <p v-else class="hint">{{ info.displayName }} has no permission mode Chorus can pass at launch yet.</p>
            <p v-if="permissionOptions.length && permissionShown === null" class="hint">None chosen: {{ info.displayName }}'s own setting applies.</p>
          </div>
        </section>

        <section v-if="modelUpFront" class="pair">
          <label class="field">
            <span class="label">Runs on</span>
            <select v-model="providerId">
              <option :value="null">{{ SIGN_IN[agent] }}</option>
              <option v-for="r in routes" :key="r.provider.id" :value="r.provider.id" :disabled="r.reason !== null">
                {{ r.provider.name }}{{ r.reason ? ` (not for ${info.displayName}: ${r.reason})` : '' }}
              </option>
            </select>
          </label>
          <label class="field">
            <span class="label">Model <em>{{ provider ? '(required)' : '(optional)' }}</em></span>
            <input
              v-model="model"
              class="mono"
              maxlength="200"
              spellcheck="false"
              autocomplete="off"
              list="provider-shortlist"
              :placeholder="provider ? (provider.shortlist[0] ?? 'author/model') : 'provider/model, for example opencode/big-pickle'"
            />
          </label>
          <span v-if="keyNote" class="hint span">{{ keyNote }}</span>
        </section>

        <RoutingTiers v-if="routingApplies && providerId !== null" v-model="routing" :provider-id="providerId" :model="model" :effort="effortShown" />

        <section ref="runsOnEl" class="runs-on">
          <button type="button" class="quiet disclosure" :aria-expanded="runsOnOpen" @click="runsOnOpen = !runsOnOpen">
            <span class="caret" :class="{ open: runsOnOpen }">›</span>
            Runs on {{ runsOnText }}
          </button>
          <div v-if="runsOnOpen" class="advanced">
            <label v-if="!modelUpFront" class="field">
              <span class="label">Model <em>{{ provider ? `(required: a ${provider.name} model id)` : "(optional: the CLI's default otherwise)" }}</em></span>
              <input v-model="model" class="mono" maxlength="200" spellcheck="false" autocomplete="off" list="provider-shortlist" :placeholder="provider?.shortlist[0] ?? ''" />
            </label>
            <label v-if="routes.length && !modelUpFront" class="field">
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
        </section>
        <datalist id="provider-shortlist">
          <option v-for="m in provider?.shortlist ?? []" :key="m" :value="m" />
        </datalist>
      </template>
    </div>

    <aside class="summary scroll" aria-label="Summary">
      <div class="summary-head">
        <span class="caps">Summary</span>
        <button type="button" class="quiet x" aria-label="Close" title="Close (Esc)" :disabled="busy" @click="emit('cancel')"><Icon name="close" :size="16" /></button>
      </div>

      <div class="field">
        <span class="small">Mode</span>
        <ButtonGroup v-model="preset" :options="presetOptions" label="Mode" />
        <div v-if="preset === 'swarm'" class="count">
          <span class="small">Agents</span>
          <ButtonGroup v-model="swarmCount" :options="countOptions" label="How many agents" />
        </div>
        <p class="small">{{ presetText }}</p>
      </div>

      <dl class="facts">
        <dt>Agent</dt>
        <dd>{{ info?.displayName ?? AGENT_LABELS[agent] }}</dd>
        <template v-if="version">
          <dt>Version</dt>
          <dd :class="{ warn: version.warn }" :title="version.title">{{ version.text }}</dd>
        </template>
        <dt>Branch</dt>
        <dd v-if="git" class="mono">{{ git.repo }} / {{ branchText }}</dd>
        <dd v-else class="muted">{{ check ? 'not a git repository' : '—' }}</dd>
        <template v-if="git">
          <dt>Works in</dt>
          <dd>{{ preset === 'swarm' ? `${swarmCount} new worktrees` : workspace === 'new-worktree' ? 'a new worktree' : 'this folder' }}</dd>
        </template>
        <dt>Name</dt>
        <dd>{{ name.trim() || 'unnamed' }}</dd>
        <dt>Project</dt>
        <dd :class="{ muted: !chosenProject }">{{ chosenProject?.name ?? `By folder${autoProject ? `: ${autoProject.name}` : ''}` }}</dd>
        <template v-if="tags.length">
          <dt>Tags</dt>
          <dd>{{ tags.join(', ') }}</dd>
        </template>
        <template v-if="routingApplies && routing">
          <dt>Routing</dt>
          <dd>{{ ROUTING_LABELS[routing] }}</dd>
        </template>
      </dl>

      <div v-if="agent !== 'shell'" class="field">
        <span class="small">{{ modelUpFront ? 'Server config' : 'Command' }}</span>
        <div class="command mono">
          <span class="program">{{ PROGRAMS[agent] }}</span>{{ args.length ? ` ${args.join(' ')}` : '' }}
          <span v-if="!args.length" class="muted">{{ ' (its own defaults)' }}</span>
          <span v-if="provider" class="muted">{{ ` + ${provider.name}'s settings` }}</span>
        </div>
      </div>

      <div class="spacer" />

      <button type="button" class="quiet runs" :title="'What it runs on: change it in the form'" @click="openRunsOn">
        <span class="led" :class="{ key: provider || keyLabel }" />Runs on {{ runsOnText }}
      </button>
      <p v-if="blocker && cwd.trim()" class="small warn">{{ blocker }}</p>
      <div class="buttons">
        <button type="submit" class="primary start" :disabled="!!blocker || busy" title="Start (Ctrl+Enter)">
          {{ busy ? 'Starting…' : sessionCount > 1 ? `Start ${sessionCount} sessions` : 'Start session' }}
        </button>
        <button type="button" :disabled="busy" @click="emit('cancel')">Cancel</button>
      </div>
    </aside>
  </form>
</template>

<style scoped>
/* A fixed height, so each pane scrolls on its own and Start stays in view. */
.box {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 280px;
  grid-template-rows: 100%;
  height: min(800px, calc(100vh - 48px));
}
.form {
  display: flex;
  flex-direction: column;
  gap: 20px;
  min-width: 0;
  min-height: 0;
  padding: 24px 28px 22px;
  overflow: auto;
  border-right: 1px solid var(--border);
}
h2 {
  margin: 0;
  font-family: var(--font-heading);
  font-size: 19px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}
.label {
  color: var(--text-2);
  font-size: 13px;
  font-weight: 600;
}
em {
  color: var(--text-muted);
  font-style: normal;
  font-weight: 500;
}
.small {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
}
.hint {
  margin: 0;
  color: var(--text-muted);
  font-size: 12.5px;
}
.warn {
  color: var(--status-needs);
}
.hint.bad {
  color: var(--status-failed);
}
.mono {
  font-family: var(--font-mono);
  font-size: 12px;
}
.muted {
  color: var(--text-muted);
}
.pair {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}
.pair .span {
  grid-column: 1 / -1;
}
.row {
  display: flex;
  gap: 8px;
}
.row input {
  flex: 1;
}

/* A field with a button inside it. */
.boxed {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 42px;
  padding: 0 5px 0 12px;
  border-radius: 10px;
  background: var(--surface);
  border: 1px solid var(--border-strong);
}
.boxed:focus-within {
  outline: 2px solid var(--accent-line);
  outline-offset: 1px;
}
.boxed input {
  flex: 1;
  min-width: 0;
  padding: 0;
  border: 0;
  background: transparent;
}
.boxed input:focus {
  outline: none;
}
.inner {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 32px;
  padding: 0 12px;
  border-radius: 7px;
  font-size: 12px;
  font-weight: 600;
}
.inner.icon {
  width: 32px;
  padding: 0;
  justify-content: center;
}

/* Agent logos, tinted like the text. */
.logo {
  width: 72px;
  height: 72px;
  background: var(--text-muted);
  -webkit-mask: var(--logo) center / contain no-repeat;
  mask: var(--logo) center / contain no-repeat;
}
.form :deep(button:hover:not(:disabled) .logo) {
  background: var(--text-2);
}
.form :deep(button.on .logo) {
  background: var(--accent);
}
.agent-name {
  font-size: 12px;
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

/* Tags on top, the note below, in one box. */
.tags-note {
  border-radius: 10px;
  border: 1px solid var(--border-strong);
  background: var(--surface);
  overflow: visible;
}
.tags-note:focus-within {
  outline: 2px solid var(--accent-line);
  outline-offset: 1px;
}
.tags-note :deep(.tags) {
  border: 0;
  border-bottom: 1px solid var(--border);
  border-radius: 10px 10px 0 0;
  background: transparent;
  outline: none;
  min-height: 40px;
  padding: 4px 4px 4px 8px;
}
.tags-note textarea {
  display: block;
  width: 100%;
  box-sizing: border-box;
  min-height: 56px;
  padding: 10px 12px;
  border: 0;
  border-radius: 0 0 10px 10px;
  background: transparent;
  resize: vertical;
}
.tags-note textarea:focus {
  outline: none;
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
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 10px 0 0 18px;
}

/* The summary pane. */
.summary {
  display: flex;
  flex-direction: column;
  gap: 18px;
  min-height: 0;
  padding: 24px 22px 22px;
  overflow: auto;
  background: var(--bg-sidebar);
}
.summary-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.caps {
  color: var(--text-muted);
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}
.x {
  padding: 4px;
  display: grid;
}
.count {
  display: flex;
  align-items: center;
  gap: 8px;
}
.count :deep(.group) {
  flex: 1;
}
.facts {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: 9px 12px;
  margin: 0;
  font-size: 13px;
}
.facts dt {
  color: var(--text-muted);
}
.facts dd {
  margin: 0;
  text-align: right;
  font-weight: 600;
  overflow-wrap: anywhere;
}
.facts dd.mono {
  font-weight: 400;
}
.facts dd.muted {
  font-weight: 400;
}
.command {
  padding: 10px 12px;
  border-radius: 8px;
  border: 1px solid var(--border-strong);
  background: var(--surface);
  line-height: 1.6;
  color: var(--text-2);
  overflow-wrap: anywhere;
}
.program {
  color: var(--accent);
}
.spacer {
  flex: 1;
}
.runs {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 2px 0;
  text-align: left;
  font-size: 12px;
  color: var(--text-2);
  white-space: normal;
}
.led {
  flex: none;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--status-ready);
}
.led.key {
  background: var(--status-needs);
}
.buttons {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.start {
  height: 40px;
  font-size: 14px;
}

/* Narrow windows: the summary goes under the form, and the whole dialog scrolls. */
@media (max-width: 760px) {
  .box {
    grid-template-columns: 1fr;
    grid-template-rows: auto;
    height: auto;
    max-height: calc(100vh - 48px);
    overflow: auto;
  }
  .form {
    overflow: visible;
    border-right: 0;
    border-bottom: 1px solid var(--border);
  }
  .summary {
    overflow: visible;
  }
  .pair {
    grid-template-columns: 1fr;
  }
}
</style>
