<script setup lang="ts">
/**
 * Settings → Council (P10, D136–D137): the OpenRouter provider whose management key caps each
 * run, the members and the arbiter, and how much code each turn may read. A member whose model
 * can't take tools is shown with the reason and keeps the council from running: it couldn't read
 * the code.
 */
import { computed, onMounted, ref } from 'vue'
import { commands, unwrap, type CatalogEntry, type CouncilLimits, type CouncilMemberView, type CouncilRole, type CouncilSettings } from '../../ipc'
import { useViewStore } from '../../stores/view'

const view = useViewStore()
const settings = ref<CouncilSettings | null>(null)
const catalog = ref<CatalogEntry[]>([])
const error = ref('')
const notice = ref('')
const busy = ref(false)

const providerId = ref<number | null>(null)
/** The limits as edited: kilobytes and percent, the units people think in. */
const calls = ref(40)
const kb = ref(1500)
const contextPct = ref(70)
const cap = ref(10)

interface Draft {
  id: number | null
  label: string
  model: string
  role: CouncilRole
  maxTokens: string
}
const draft = ref<Draft | null>(null)
const search = ref('')
const confirmDelete = ref<number | null>(null)

function fillLimits(l: CouncilLimits): void {
  calls.value = l.maxCalls
  kb.value = Math.round(l.maxBytes / 1000)
  contextPct.value = Math.round((l.contextShare ?? 0.7) * 100)
  cap.value = l.capUsd ?? 10
}

async function loadCatalog(): Promise<void> {
  catalog.value = providerId.value ? await unwrap(commands.providerCatalog(providerId.value)) : []
}

async function load(): Promise<void> {
  try {
    const s = await unwrap(commands.councilSettings())
    settings.value = s
    providerId.value = s.config.providerId
    fillLimits(s.config.limits)
    await loadCatalog()
  } catch (e) {
    error.value = String(e)
  }
}
onMounted(load)

/** Models that take tools, cheapest input first, filtered by the search. */
const models = computed(() =>
  catalog.value
    .filter((c) => !c.missing && c.model.tools === true)
    .filter((c) => `${c.model.id} ${c.model.name}`.toLowerCase().includes(search.value.toLowerCase()))
    .sort((a, b) => (a.model.promptPerMtok ?? Infinity) - (b.model.promptPerMtok ?? Infinity))
    .slice(0, 80),
)

const arbiters = computed(() => settings.value?.members.filter((m) => m.member.role === 'arbiter') ?? [])
const deliberators = computed(() => settings.value?.members.filter((m) => m.member.role === 'member') ?? [])

async function saveConfig(): Promise<void> {
  busy.value = true
  error.value = ''
  notice.value = ''
  try {
    settings.value = await unwrap(
      commands.councilConfigure({
        providerId: providerId.value,
        limits: { maxCalls: calls.value, maxBytes: kb.value * 1000, contextShare: contextPct.value / 100, capUsd: cap.value },
      }),
    )
    await loadCatalog()
    notice.value = 'Saved. The next council uses these settings; one already running keeps its own.'
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
  }
}

function resetLimits(): void {
  if (settings.value) fillLimits(settings.value.defaultLimits)
}

function startAdd(role: CouncilRole): void {
  draft.value = { id: null, label: '', model: '', role, maxTokens: '' }
  search.value = ''
}

function startEdit(m: CouncilMemberView): void {
  draft.value = { id: m.member.id, label: m.member.label, model: m.member.model, role: m.member.role, maxTokens: m.member.maxTokens?.toString() ?? '' }
  search.value = ''
}

async function saveMember(): Promise<void> {
  if (!draft.value) return
  const d = draft.value
  const max = d.maxTokens.trim()
  if (max && !/^\d+$/.test(max)) {
    error.value = 'Max output tokens must be a whole number, or empty for the default.'
    return
  }
  busy.value = true
  error.value = ''
  try {
    settings.value = await unwrap(
      commands.councilMemberSave({ id: d.id, label: d.label, model: d.model, role: d.role, maxTokens: max ? Number(max) : null }),
    )
    draft.value = null
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
  }
}

async function remove(id: number): Promise<void> {
  busy.value = true
  error.value = ''
  try {
    settings.value = await unwrap(commands.councilMemberDelete(id))
    confirmDelete.value = null
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
  }
}

async function refreshCatalog(): Promise<void> {
  if (!providerId.value) return
  busy.value = true
  error.value = ''
  try {
    await unwrap(commands.providerRefreshCatalog(providerId.value))
    await loadCatalog()
    settings.value = await unwrap(commands.councilSettings())
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
  }
}

function price(c: CatalogEntry): string {
  const p = (n: number | null) => (n === null ? '?' : `$${n}`)
  return `${p(c.model.promptPerMtok)} / ${p(c.model.completionPerMtok)} per M tokens`
}

function context(n: number | null): string {
  return n === null ? '' : ` · ${Math.round(n / 1000)}k context`
}
</script>

<template>
  <section class="council-settings">
    <h2>Council</h2>
    <p>
      A council reviews a brief: its members answer the brief's numbered questions blind, critique each other anonymously, and the arbiter rules on each
      question and writes the findings. Every participant can read the project's code, read-only, to check the brief against it.
    </p>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-if="notice" role="status">{{ notice }}</p>
    <p v-if="settings?.reason" class="warn" role="status">{{ settings.reason }}</p>
    <p v-else-if="settings" class="ok" role="status">Ready: {{ deliberators.length }} members and an arbiter.</p>

    <form class="block" @submit.prevent="saveConfig">
      <h3>Provider and limits</h3>
      <label>
        OpenRouter provider
        <select v-model="providerId" :disabled="busy" @change="loadCatalog">
          <option :value="null">Not chosen</option>
          <option v-for="p in settings?.providers ?? []" :key="p.id" :value="p.id">{{ p.name }}{{ p.problem ? ` — ${p.problem}` : '' }}</option>
        </select>
      </label>
      <p class="muted">
        Each run mints its own OpenRouter key from this provider's management key, capped at the spend limit below, and revokes it when the run ends.
        <button type="button" class="link" @click="view.openSettings('providers')">Manage providers and keys</button>
      </p>
      <div class="limits">
        <label>Spend cap per run ($)<input v-model.number="cap" type="number" min="0.5" max="100" step="0.5" :disabled="busy" /></label>
        <label>Tool calls per turn<input v-model.number="calls" type="number" min="1" max="200" :disabled="busy" /></label>
        <label>Code read per turn (KB)<input v-model.number="kb" type="number" min="10" max="8000" step="50" :disabled="busy" /></label>
        <label>Stop reading at (% of context)<input v-model.number="contextPct" type="number" min="20" max="95" :disabled="busy" /></label>
      </div>
      <p class="muted">
        When a turn reaches any of these, its model is told to answer with what it has read. The context limit stops a long conversation before the model's
        own limit refuses it.
      </p>
      <div class="row">
        <button type="submit" class="primary" :disabled="busy">Save</button>
        <button type="button" :disabled="busy" @click="resetLimits">Default limits</button>
      </div>
    </form>

    <div class="block">
      <h3>Members</h3>
      <p class="muted">At least two members and exactly one arbiter, at most twelve in all. Each must be a model that takes tools, so it can read the code.</p>
      <ul class="members">
        <li v-for="m in [...arbiters, ...deliberators]" :key="m.member.id" :class="{ bad: m.problem }">
          <div class="who">
            <strong>{{ m.member.label }}</strong>
            <span class="chip" :class="m.member.role">{{ m.member.role }}</span>
          </div>
          <div class="mono">{{ m.member.model }}</div>
          <div class="muted small">
            {{ m.effectiveMaxTokens.toLocaleString() }} output tokens{{ m.member.maxTokens === null ? ' (default)' : '' }}{{ context(m.contextLength) }}
          </div>
          <p v-if="m.problem" class="error small">{{ m.problem }}</p>
          <div class="row">
            <button type="button" :disabled="busy" @click="startEdit(m)">Edit</button>
            <template v-if="confirmDelete === m.member.id">
              <button type="button" class="danger" :disabled="busy" @click="remove(m.member.id)">Remove {{ m.member.label }}</button>
              <button type="button" @click="confirmDelete = null">Keep</button>
            </template>
            <button v-else type="button" :disabled="busy" @click="confirmDelete = m.member.id">Remove</button>
          </div>
        </li>
      </ul>
      <p v-if="settings && !settings.members.length" class="muted">No members yet.</p>
      <div v-if="!draft" class="row">
        <button type="button" :disabled="busy" @click="startAdd('member')">Add member</button>
        <button type="button" :disabled="busy || arbiters.length > 0" :title="arbiters.length ? 'There is already an arbiter' : ''" @click="startAdd('arbiter')">
          Add arbiter
        </button>
      </div>

      <form v-if="draft" class="editor" @submit.prevent="saveMember">
        <h4>{{ draft.id === null ? `New ${draft.role}` : `Edit ${draft.label}` }}</h4>
        <div class="limits">
          <label>Name<input v-model="draft.label" maxlength="60" placeholder="e.g. Opus" :disabled="busy" /></label>
          <label>
            Role
            <select v-model="draft.role" :disabled="busy">
              <option value="member">member — answers and critiques</option>
              <option value="arbiter">arbiter — rules and writes the findings</option>
            </select>
          </label>
          <label>Max output tokens<input v-model="draft.maxTokens" inputmode="numeric" :placeholder="draft.role === 'arbiter' ? '32000 (default)' : '16000 (default)'" :disabled="busy" /></label>
        </div>
        <label>Model id<input v-model="draft.model" class="mono" maxlength="200" placeholder="Choose below, or type an OpenRouter model id" spellcheck="false" :disabled="busy" /></label>
        <details v-if="providerId" open>
          <summary>Models on this provider that take tools</summary>
          <div class="row">
            <input v-model="search" placeholder="Search models…" aria-label="Search council models" />
            <button type="button" :disabled="busy" @click="refreshCatalog">Refresh models</button>
          </div>
          <div class="models">
            <button v-for="c in models" :key="c.model.id" type="button" :class="{ on: c.model.id === draft.model }" @click="draft.model = c.model.id">
              {{ c.model.name }}
              <small>{{ c.model.id }} · {{ price(c) }}{{ context(c.model.contextLength) }}</small>
            </button>
          </div>
          <p v-if="!models.length" class="muted">No matching models that take tools. Refresh the list, or change the search.</p>
        </details>
        <p v-else class="muted">Choose and save the provider above to pick from its models.</p>
        <div class="row">
          <button type="submit" class="primary" :disabled="busy || !draft.label.trim() || !draft.model.trim()">Save {{ draft.role }}</button>
          <button type="button" :disabled="busy" @click="draft = null">Cancel</button>
        </div>
      </form>
    </div>
  </section>
</template>

<style scoped>
h2 {
  margin-top: 0;
}
h3 {
  margin: 0 0 10px;
  font-size: 15px;
}
h4 {
  margin: 0;
  font-size: 14px;
}
p {
  font-size: 13px;
  line-height: 1.6;
}
.muted {
  color: var(--text-muted);
}
.small {
  font-size: 12px;
}
.error {
  color: var(--status-failed);
}
.warn {
  color: var(--status-needs);
}
.ok {
  color: var(--status-done);
}
.block {
  display: grid;
  gap: 12px;
  margin: 24px 0;
}
label {
  display: grid;
  gap: 6px;
  font-size: 13px;
}
select,
input {
  padding: 8px 10px;
  width: 100%;
}
.limits {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
  gap: 12px;
}
.row {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.row input {
  flex: 1;
  width: auto;
}
button {
  width: fit-content;
  padding: 7px 12px;
}
button.link {
  border: none;
  background: none;
  color: var(--accent);
  padding: 0;
  cursor: pointer;
}
.members {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
  gap: 10px;
}
.members li {
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  padding: 12px;
  display: grid;
  gap: 6px;
  background: var(--surface-raised);
}
.members li.bad {
  border-color: var(--status-failed);
}
.who {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.chip {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: var(--radius-pill);
  background: var(--surface-chip);
}
.chip.arbiter {
  background: var(--accent-soft);
  color: var(--accent);
}
.mono {
  font-family: var(--font-mono);
  font-size: 12px;
  word-break: break-all;
}
.editor {
  display: grid;
  gap: 12px;
  padding: 16px;
  border: 1px solid var(--accent-line);
  border-radius: var(--radius-card);
  background: var(--surface-raised);
}
.models {
  max-height: 260px;
  overflow: auto;
  display: grid;
  gap: 4px;
  margin-top: 8px;
}
.models button {
  width: 100%;
  text-align: left;
}
.models button.on {
  border-color: var(--accent);
}
small {
  display: block;
  color: var(--text-muted);
  margin-top: 3px;
}
summary {
  cursor: pointer;
  font-size: 13px;
  margin-bottom: 8px;
}
.danger {
  color: var(--status-failed);
}
</style>
