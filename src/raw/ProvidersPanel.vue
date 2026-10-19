<script setup lang="ts">
/**
 * Providers: OpenRouter, OpenAI-compatible gateways and Jev. Each points at keys from the vault,
 * can be tested, and (except Jev) has a model catalog with a shortlist to launch from. Settings →
 * Providers replaces this with P4's shell.
 */
import { computed, onMounted, reactive, ref } from 'vue'
import type { CatalogEntry, ProviderKind, ProviderSummary } from '../ipc'
import { useKeysStore } from '../stores/keys'
import { useProvidersStore } from '../stores/providers'
import { AGENT_LABELS, KIND_LABELS, money, when } from './labels'

const providers = useProvidersStore()
const keys = useKeysStore()
onMounted(() => {
  void providers.load()
  void keys.load()
})

// The add or edit form. `id` null adds.
const form = reactive({
  id: null as number | null,
  kind: 'openrouter' as ProviderKind,
  name: '',
  baseUrl: '',
  credentialId: null as number | null,
  managementCredentialId: null as number | null,
  cap: '',
})
const editing = ref(false)

function startAdd(): void {
  Object.assign(form, { id: null, kind: 'openrouter', name: 'OpenRouter', baseUrl: '', credentialId: null, managementCredentialId: null, cap: '' })
  editing.value = true
}

function startEdit(p: ProviderSummary): void {
  Object.assign(form, {
    id: p.id,
    kind: p.kind,
    name: p.name,
    baseUrl: p.kind === 'gateway' ? p.baseUrl : '',
    credentialId: p.credentialId,
    managementCredentialId: p.managementCredentialId,
    cap: p.capUsd === null ? '' : String(p.capUsd),
  })
  editing.value = true
}

const capValue = computed(() => (form.cap.trim() === '' ? null : Number(form.cap)))
const formProblem = computed(() => {
  if (!form.name.trim()) return 'Give it a name.'
  if (form.kind === 'gateway' && !form.baseUrl.trim()) return 'A gateway needs its address.'
  if (form.kind !== 'openrouter' && form.credentialId === null) return 'Choose its key.'
  if (form.kind === 'openrouter' && form.credentialId === null && form.managementCredentialId === null) return 'Choose a key.'
  if (capValue.value !== null && !(capValue.value > 0)) return 'The cap is a dollar amount above 0.'
  return null
})

async function submit(): Promise<void> {
  if (formProblem.value) return
  const saved = await providers.save({
    id: form.id,
    kind: form.kind,
    name: form.name,
    baseUrl: form.kind === 'gateway' ? form.baseUrl : null,
    credentialId: form.credentialId,
    managementCredentialId: form.kind === 'openrouter' ? form.managementCredentialId : null,
    capUsd: form.kind === 'openrouter' ? capValue.value : null,
  })
  if (saved) editing.value = false
}

// The shortlist editor for one provider.
const picking = ref<ProviderSummary | null>(null)
const entries = ref<CatalogEntry[]>([])
const chosen = ref<string[]>([])
const filter = ref('')
const freeOnly = ref(false)
const toolsOnly = ref(true)
const typed = ref('')
const SHOWN = 150

async function openShortlist(p: ProviderSummary): Promise<void> {
  picking.value = p
  chosen.value = [...p.shortlist]
  entries.value = await providers.catalog(p.id)
}

const matches = computed(() => {
  const words = filter.value.toLowerCase().split(/\s+/).filter(Boolean)
  return entries.value.filter(
    (e) =>
      (!freeOnly.value || e.model.free) &&
      (!toolsOnly.value || e.model.tools !== false) &&
      words.every((w) => e.model.id.toLowerCase().includes(w) || e.model.name.toLowerCase().includes(w)),
  )
})

function toggle(id: string): void {
  chosen.value = chosen.value.includes(id) ? chosen.value.filter((x) => x !== id) : [...chosen.value, id]
}

function addTyped(): void {
  const id = typed.value.trim()
  if (id && !chosen.value.includes(id)) chosen.value = [...chosen.value, id]
  typed.value = ''
}

async function saveShortlist(): Promise<void> {
  if (picking.value && (await providers.setShortlist(picking.value.id, chosen.value))) picking.value = null
}

function supported(p: ProviderSummary): string {
  const ok = p.agents.filter((a) => a.reason === null).map((a) => AGENT_LABELS[a.agent])
  return ok.length ? ok.join(', ') : 'no agent (it answers Chorus, not an agent)'
}
</script>

<template>
  <section class="panel">
    <h2>Providers</h2>
    <p v-if="providers.error" class="error" role="alert">{{ providers.error }}</p>
    <p v-if="!providers.list.length && !editing" class="hint">
      No providers yet. Add OpenRouter with your normal key, and its management key if you want each run to get its own
      spend-capped key.
    </p>

    <article v-for="p in providers.list" :key="p.id" class="provider">
      <header>
        <strong>{{ p.name }}</strong>
        <span class="kind">{{ KIND_LABELS[p.kind] }}</span>
        <code>{{ p.baseUrl }}</code>
      </header>
      <p class="facts">
        Key: <strong>{{ p.credentialLabel ?? 'none' }}</strong>
        <template v-if="p.kind === 'openrouter'">
          · Management key: <strong>{{ p.managementLabel ?? 'none (runs use the key above, without a cap)' }}</strong>
          <template v-if="p.managementCredentialId !== null"> · Cap per run: {{ money(p.capUsd ?? p.defaultCapUsd) }}</template>
        </template>
      </p>
      <p class="facts">Runs: {{ supported(p) }}</p>
      <p class="facts" :class="p.testOk === false ? 'bad' : p.testOk ? 'good' : ''">
        <template v-if="p.testedAtSecs">Tested {{ when(p.testedAtSecs) }}: {{ p.testNote }}</template>
        <template v-else>Not tested yet.</template>
      </p>
      <p v-if="p.kind !== 'jev'" class="facts">
        Models: {{ p.catalogRefreshedAtSecs ? `${p.catalogCount} listed ${when(p.catalogRefreshedAtSecs)}` : 'not fetched yet' }} ·
        shortlist: {{ p.shortlist.length ? p.shortlist.join(', ') : 'empty' }}
      </p>
      <div class="actions">
        <button :disabled="providers.busyId !== null" @click="providers.test(p.id)">
          {{ providers.busyId === p.id ? 'Working…' : 'Test' }}
        </button>
        <button v-if="p.kind !== 'jev'" :disabled="providers.busyId !== null" @click="providers.refresh(p.id)">Refresh models</button>
        <button v-if="p.kind !== 'jev'" :disabled="!p.catalogCount && !p.shortlist.length" @click="openShortlist(p)">Shortlist…</button>
        <button @click="startEdit(p)">Edit</button>
        <button class="danger" @click="providers.remove(p.id)">Delete</button>
      </div>
      <p v-if="p.kind === 'jev'" class="hint">The test asks Jev one tiny question, which costs about $0.00001.</p>
    </article>

    <section v-if="picking" class="shortlist">
      <h3>Shortlist for {{ picking.name }}</h3>
      <div class="row">
        <input v-model="filter" placeholder="Filter models" spellcheck="false" />
        <label class="check"><input v-model="freeOnly" type="checkbox" /> Free only</label>
        <label class="check"><input v-model="toolsOnly" type="checkbox" /> Takes tools</label>
      </div>
      <p class="hint">
        {{ matches.length }} of {{ entries.length }} models{{ matches.length > SHOWN ? `, first ${SHOWN} shown` : '' }}. Agents need
        models that take tools.
      </p>
      <ul class="models">
        <li v-for="e in matches.slice(0, SHOWN)" :key="e.model.id">
          <label>
            <input type="checkbox" :checked="chosen.includes(e.model.id)" @change="toggle(e.model.id)" />
            <code>{{ e.model.id }}</code>
            <span class="muted">
              {{ e.model.free ? 'free' : `${money(e.model.promptPerMtok)} / ${money(e.model.completionPerMtok)} per M` }}
              <template v-if="e.model.contextLength"> · {{ Math.round(e.model.contextLength / 1000) }}k</template>
              <template v-if="e.model.efforts.length"> · effort {{ e.model.efforts.join('/') }}</template>
              <template v-if="e.missing"> · no longer listed</template>
            </span>
          </label>
        </li>
      </ul>
      <form class="row" @submit.prevent="addTyped">
        <input v-model="typed" placeholder="Or type a model id" spellcheck="false" />
        <button type="submit" :disabled="!typed.trim()">Add</button>
      </form>
      <p class="hint">Chosen: {{ chosen.length ? chosen.join(', ') : 'none' }}</p>
      <div class="actions">
        <button class="primary" @click="saveShortlist">Save shortlist</button>
        <button @click="picking = null">Cancel</button>
      </div>
    </section>

    <form v-if="editing" class="edit" @submit.prevent="submit">
      <h3>{{ form.id === null ? 'Add a provider' : `Edit ${form.name}` }}</h3>
      <label>
        <span>Kind</span>
        <select v-model="form.kind" :disabled="form.id !== null">
          <option v-for="(label, k) in KIND_LABELS" :key="k" :value="k">{{ label }}</option>
        </select>
      </label>
      <label>
        <span>Name</span>
        <input v-model="form.name" maxlength="60" autocomplete="off" />
      </label>
      <label v-if="form.kind === 'gateway'">
        <span>Address (OpenAI-compatible, with the Responses API)</span>
        <input v-model="form.baseUrl" placeholder="https://gateway.example.com/v1" spellcheck="false" autocomplete="off" />
      </label>
      <label>
        <span>Key <em>{{ form.kind === 'openrouter' ? '(your normal OpenRouter key)' : '' }}</em></span>
        <select v-model="form.credentialId">
          <option :value="null">None</option>
          <option v-for="k in keys.keys" :key="k.id" :value="k.id">{{ k.label }} · {{ k.fingerprint }}</option>
        </select>
      </label>
      <template v-if="form.kind === 'openrouter'">
        <label>
          <span>Management key <em>(mints a capped key for each run; it can't run models itself)</em></span>
          <select v-model="form.managementCredentialId">
            <option :value="null">None</option>
            <option v-for="k in keys.keys" :key="k.id" :value="k.id">{{ k.label }} · {{ k.fingerprint }}</option>
          </select>
        </label>
        <label v-if="form.managementCredentialId !== null">
          <span>Cap per run, in dollars <em>(default $1.00; OpenRouter holds back a request's full output cost, so less can refuse Codex)</em></span>
          <input v-model="form.cap" inputmode="decimal" placeholder="1.00" autocomplete="off" />
        </label>
      </template>
      <p v-if="!keys.keys.length" class="hint warn">Add the keys under Keys and lock first.</p>
      <p v-if="formProblem" class="hint">{{ formProblem }}</p>
      <div class="actions">
        <button type="submit" class="primary" :disabled="formProblem !== null">Save</button>
        <button type="button" @click="editing = false">Cancel</button>
      </div>
    </form>
    <div v-else class="actions">
      <button @click="startAdd">Add a provider</button>
    </div>
  </section>
</template>

<style scoped>
.panel {
  max-width: 900px;
  display: grid;
  gap: 14px;
}
h2,
h3 {
  font-family: var(--font-heading);
  margin: 8px 0 0;
}
h2 {
  font-size: 20px;
}
h3 {
  font-size: 16px;
}
.provider,
.shortlist,
.edit {
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 12px 14px;
  display: grid;
  gap: 8px;
}
.provider header {
  display: flex;
  gap: 10px;
  align-items: baseline;
  flex-wrap: wrap;
}
.kind,
.muted,
em {
  color: var(--text-muted);
  font-style: normal;
}
.facts {
  margin: 0;
  font-size: 13px;
  color: var(--text-2);
}
.facts.good {
  color: var(--status-ready);
}
.facts.bad,
.error {
  color: var(--status-failed);
  margin: 0;
}
.actions,
.row {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
  align-items: center;
}
.row input:not([type='checkbox']) {
  flex: 1 1 220px;
}
.check {
  display: flex;
  gap: 6px;
  align-items: center;
  font-size: 13px;
}
.models {
  list-style: none;
  margin: 0;
  padding: 0;
  max-height: 360px;
  overflow: auto;
  display: grid;
  gap: 4px;
}
.models label {
  display: flex;
  gap: 8px;
  align-items: baseline;
  flex-wrap: wrap;
  font-size: 13px;
}
.edit label {
  display: grid;
  gap: 6px;
}
.edit label span {
  color: var(--text-2);
  font-size: 13px;
}
.hint {
  color: var(--text-muted);
  margin: 0;
  font-size: 12.5px;
}
.hint.warn {
  color: var(--status-needs);
}
code {
  font-family: var(--font-mono);
  font-size: 12.5px;
}
</style>
