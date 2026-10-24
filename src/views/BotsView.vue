<script setup lang="ts">
/**
 * Bots (P8, decision 1): the saved launch profiles, grown. A card per Bot with its agent, model,
 * description, tags and usage; Edit opens the editor, Start session opens New session filled
 * from the Bot. Editing a Bot never changes sessions already started from it.
 */
import { computed, onMounted, ref } from 'vue'
import { commands, unwrap, type Bot, type BotSave, type Effort, type Permission, type Preset } from '../ipc'
import TagInput from '../components/TagInput.vue'
import { useBotsStore } from '../stores/bots'
import { useAgentsStore } from '../stores/agents'
import { AGENT_CHIPS } from '../shell/status'
import { botUsage } from './botText'
import { AGENT_LABELS, EFFORT_LABELS, PERMISSION_LABELS } from '../raw/labels'

const bots = useBotsStore()
const agents = useAgentsStore()

const editing = ref<BotSave | null>(null)
const skillsText = ref('')
const error = ref<string | null>(null)
const busy = ref(false)
const knownTags = ref<string[]>([])

onMounted(async () => {
  void bots.load()
  if (!agents.list.length) void agents.load(false)
  const r = await commands.tagsKnown().catch(() => null)
  if (r?.status === 'ok') knownTags.value = r.data.map((t) => t.tag)
})

const active = computed(() => bots.list.filter((b) => !b.archived))
const archived = computed(() => bots.list.filter((b) => b.archived))
const info = computed(() => (editing.value ? agents.byKind.get(editing.value.agent) : undefined))
const instructionsNote = computed(() => {
  if (!editing.value) return ''
  switch (editing.value.agent) {
    case 'claude':
      return 'Claude Code gets these with each run, from a file in Chorus’s own data folder.'
    case 'codex':
      return 'Codex 0.163.0 gets these through Chorus’s private Codex server, as a developer message at the start of the conversation. Other Codex versions don’t get them: the other ways put them on the command line.'
    case 'shell':
      return 'A shell has no agent instructions.'
    default:
      return `${AGENT_LABELS[editing.value.agent]} has no verified way to take instructions from Chorus, so these aren’t given to it.`
  }
})

const PRESETS: { value: Preset | null; label: string }[] = [
  { value: null, label: 'Solo' },
  { value: 'pair', label: 'Pair' },
  { value: 'workbench', label: 'Workbench' },
  { value: 'swarm', label: 'Swarm' },
]

function usage(b: Bot): string {
  return botUsage(b)
}

function startNew(): void {
  error.value = null
  skillsText.value = ''
  editing.value = {
    id: null,
    name: '',
    agent: 'claude',
    options: { effort: null, permission: null, model: null },
    providerId: null,
    credential: null,
    description: null,
    instructions: '',
    tags: [],
    skills: [],
    preset: null,
  }
}

function edit(b: Bot): void {
  error.value = null
  skillsText.value = b.skills.join('\n')
  editing.value = {
    id: b.id,
    name: b.name,
    agent: b.agent,
    options: { ...b.options },
    providerId: b.providerId,
    credential: b.credential,
    description: b.description,
    instructions: b.instructions,
    tags: [...b.tags],
    skills: [...b.skills],
    preset: b.preset,
  }
}

async function save(): Promise<void> {
  if (!editing.value) return
  busy.value = true
  error.value = null
  try {
    const input: BotSave = {
      ...editing.value,
      options: { ...editing.value.options, model: editing.value.options.model?.trim() || null },
      description: editing.value.description?.trim() || null,
      skills: skillsText.value.split('\n').map((s) => s.trim()).filter(Boolean),
    }
    await unwrap(commands.botSave(input))
    editing.value = null
    await bots.load()
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = false
  }
}

async function setArchived(b: Bot, value: boolean): Promise<void> {
  try {
    await unwrap(commands.botSetArchived(b.id, value))
    if (editing.value?.id === b.id) editing.value = null
    await bots.load()
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
}

function archiveEditing(): void {
  const b = bots.list.find((x) => x.id === editing.value?.id)
  if (b) void setArchived(b, true)
}

function onAgent(): void {
  if (editing.value) editing.value.options = { effort: null, permission: null, model: null }
}
</script>

<template>
  <section class="page">
    <header class="head">
      <div>
        <h1>Bots</h1>
        <p class="lead">
          A Bot is a reusable configuration: agent, model, permissions and instructions. Starting one creates a new session; editing a Bot never
          changes sessions that are already running.
        </p>
      </div>
      <button type="button" class="primary" @click="startNew">New Bot</button>
    </header>

    <form v-if="editing" class="editor" @submit.prevent="save">
      <div class="grid">
        <label>Name <input v-model="editing.name" maxlength="80" autocomplete="off" /></label>
        <label>
          Agent
          <select v-model="editing.agent" @change="onAgent">
            <option v-for="a in agents.list" :key="a.kind" :value="a.kind">{{ a.displayName }}</option>
          </select>
        </label>
        <label v-if="info?.takesModel">Model <input v-model="editing.options.model" placeholder="The CLI's own choice" autocomplete="off" /></label>
        <label v-if="info?.efforts.length">
          Effort
          <select v-model="editing.options.effort">
            <option :value="null">Default</option>
            <option v-for="e in info.efforts" :key="e.effort" :value="e.effort as Effort">{{ EFFORT_LABELS[e.effort] }}</option>
          </select>
        </label>
        <label v-if="info?.permissions.length">
          Permissions
          <select v-model="editing.options.permission">
            <option :value="null">Default</option>
            <option v-for="p in info.permissions" :key="p.permission" :value="p.permission as Permission">{{ PERMISSION_LABELS[p.permission] }}</option>
          </select>
        </label>
        <label>
          Preset
          <select v-model="editing.preset">
            <option v-for="p in PRESETS" :key="p.label" :value="p.value">{{ p.label }}</option>
          </select>
        </label>
      </div>
      <label>Description <input v-model="editing.description" placeholder="What it is for" maxlength="2000" autocomplete="off" /></label>
      <label>Tags <TagInput v-model="editing.tags" :known="knownTags" label="Tags" /></label>
      <label>
        Instructions (Markdown)
        <textarea v-model="editing.instructions" rows="7" placeholder="How this Bot should work. Added to the agent's instructions at each run." />
      </label>
      <p class="hint">{{ instructionsNote }} Keys don't belong here: instructions that hold one are refused.</p>
      <label>
        Skill folders, one per line
        <textarea v-model="skillsText" rows="2" placeholder="A folder holding SKILL.md (front matter: name, description)" />
      </label>
      <p class="hint">Each run's instructions list the skills, with their folders, and tell the agent to read SKILL.md before using one.</p>
      <p v-if="editing.providerId !== null || editing.credential" class="hint">
        Runs on a provider or with a stored key, kept as saved. Change those from New session, then Save as Bot.
      </p>
      <p v-if="error" class="hint warn">{{ error }}</p>
      <div class="actions">
        <button v-if="editing.id !== null" type="button" class="quiet" title="It leaves New session and can't be started; its sessions keep running." @click="archiveEditing">Archive</button>
        <span class="grow" />
        <button type="button" class="quiet" @click="editing = null">Cancel</button>
        <button type="submit" class="primary" :disabled="busy || !editing.name.trim()">Save Bot</button>
      </div>
    </form>

    <div class="cards">
      <article v-for="b in active" :key="b.id" class="card">
        <div class="top">
          <span class="agent-chip" :class="`agent-${b.agent}`">{{ AGENT_CHIPS[b.agent] }}</span>
          <span class="agent">{{ AGENT_LABELS[b.agent] }}{{ b.providerName ? ` · ${b.providerName}` : '' }}</span>
          <span class="model">{{ b.options.model ?? '' }}</span>
        </div>
        <h2>{{ b.name }}</h2>
        <p class="desc">{{ b.description ?? (b.instructions ? b.instructions.split('\n')[0] : 'No description.') }}</p>
        <div v-if="b.tags.length || b.instructions || b.skills.length" class="tags">
          <span v-for="t in b.tags" :key="t" class="tag">{{ t }}</span>
          <span v-if="b.instructions" class="tag soft">Instructions</span>
          <span v-if="b.skills.length" class="tag soft">{{ b.skills.length }} skill{{ b.skills.length === 1 ? '' : 's' }}</span>
          <span v-if="b.preset && b.preset !== 'solo'" class="tag soft">{{ b.preset }}</span>
        </div>
        <footer>
          <span class="usage">{{ usage(b) }}</span>
          <button type="button" @click="edit(b)">Edit</button>
          <button type="button" class="primary" @click="bots.start(b)">Start session</button>
        </footer>
      </article>
    </div>
    <p v-if="!bots.list.length" class="empty">No Bots yet. Make one here, or save New session's settings as a Bot.</p>

    <template v-if="archived.length">
      <h3>Archived</h3>
      <ul class="archived">
        <li v-for="b in archived" :key="b.id">
          <span>{{ b.name }}</span>
          <span class="usage">{{ usage(b) }}</span>
          <button type="button" class="quiet" @click="setArchived(b, false)">Restore</button>
        </li>
      </ul>
    </template>
  </section>
</template>

<style scoped>
.page {
  padding: 28px 40px;
}
.head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 20px;
}
h1 {
  margin: 0 0 6px;
  font-size: 28px;
}
.lead {
  margin: 0 0 22px;
  color: var(--text-2);
  max-width: 640px;
  line-height: 1.45;
}
.editor {
  display: grid;
  gap: 10px;
  max-width: 980px;
  padding: 16px;
  border: 1px solid var(--accent-line);
  border-radius: var(--radius-card);
  background: var(--surface-raised);
  margin-bottom: 22px;
}
.editor label {
  display: grid;
  gap: 4px;
  font-size: 12.5px;
  color: var(--text-2);
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: 10px;
}
textarea {
  resize: vertical;
  font-family: inherit;
  line-height: 1.45;
}
.hint {
  margin: 0;
  font-size: 12.5px;
  color: var(--text-muted);
}
.hint.warn {
  color: var(--status-failed);
}
.actions {
  display: flex;
  gap: 8px;
}
.grow {
  flex: 1;
}
.cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: 16px;
  max-width: 1180px;
}
.card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 18px 20px;
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  background: var(--surface);
}
.top {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}
.model {
  margin-left: auto;
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-muted);
}
h2 {
  margin: 0;
  font-size: 17px;
}
.desc {
  margin: 0;
  color: var(--text-2);
  line-height: 1.45;
  font-size: 14px;
}
.tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.tag {
  padding: 2px 8px;
  border-radius: 6px;
  background: var(--surface-chip);
  font-size: 12px;
}
.tag.soft {
  color: var(--text-muted);
}
footer {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: auto;
  padding-top: 12px;
  border-top: 1px solid var(--border);
}
.usage {
  flex: 1;
  font-size: 12.5px;
  color: var(--text-muted);
}
.empty {
  color: var(--text-muted);
}
h3 {
  margin: 28px 0 8px;
  font-size: 14px;
  color: var(--text-2);
}
.archived {
  list-style: none;
  padding: 0;
  margin: 0;
  max-width: 640px;
}
.archived li {
  display: flex;
  gap: 12px;
  align-items: center;
  padding: 6px 0;
}
</style>
