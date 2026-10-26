<script setup lang="ts">
/**
 * Settings → Routing, part 1: how OpenRouter's endpoints rank one model for opencode's tiers, as
 * `routing_tiers` returns it (the same ranking New session uses). It never chooses a tier for a
 * launch. Refresh is the existing `routing_refresh`: free checks, then at most $0.05 testing
 * prompt caching. It reloads when `version` changes (the ranking settings were saved).
 */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { commands, type Effort, type ProviderSummary, type RefreshOutcome, type TiersView } from '../../ipc'
import type { ObserverView } from '../../ipc/bindings'
import { useViewStore } from '../../stores/view'
import { tiersBlocked } from '../newSession/routingFormat'
import {
  DEFAULT_CARD,
  DEFAULT_LABEL,
  EFFORT_CHOICES,
  NO_INPUT,
  REFRESHING,
  candidateHeading,
  candidateRows,
  defaultProviderId,
  effortLabel,
  inspectorProviders,
  modelSuggestions,
  nitroCard,
  refreshState,
  refreshText,
  registryLine,
  tierCards,
  updatedText,
  type InspectInput,
} from './routingInspect'

const props = defineProps<{ version: number }>()
const emit = defineEmits<{ inspect: [input: InspectInput]; refreshed: [] }>()
const nav = useViewStore()

const providers = ref<ProviderSummary[]>([])
const observer = ref<ObserverView | null>(null)
// The saved limit for a ranking's age; null when the saved settings can't be read.
const maxAge = ref<number | null>(null)

const providerId = ref<number | null>(null)
const model = ref('')
const effort = ref<Effort | null>(null)

const view = ref<TiersView | null>(null)
const loadError = ref<string | null>(null)
// A failed Refresh's message stays until the next Refresh or a change of inputs.
const refreshError = ref<string | null>(null)
const refreshed = ref<RefreshOutcome | null>(null)
const busy = ref(false)
const now = ref(Date.now())
const ticker = setInterval(() => (now.value = Date.now()), 15_000)
let timer: ReturnType<typeof setTimeout> | undefined
onUnmounted(() => {
  clearInterval(ticker)
  clearTimeout(timer)
})

const failed = (e: unknown) => ({ status: 'error' as const, error: String(e) })

async function loadContext(): Promise<void> {
  const [p, o] = await Promise.all([commands.providersList().catch(failed), commands.routingObserver().catch(failed)])
  providers.value = p.status === 'ok' ? p.data : []
  observer.value = o.status === 'ok' ? o.data : null
  // Keep the chosen provider while it exists; otherwise start with the watched or first one.
  if (!inspectorProviders(providers.value).some((x) => x.id === providerId.value)) {
    providerId.value = defaultProviderId(providers.value, observer.value?.providerId ?? null)
  }
}

let seq = 0
async function load(): Promise<void> {
  const mine = ++seq
  const input: InspectInput = { providerId: providerId.value, model: model.value.trim(), effort: effort.value }
  emit('inspect', input)
  if (input.providerId === null || !input.model) {
    view.value = null
    loadError.value = null
    return
  }
  const [tiers, settings] = await Promise.all([
    commands.routingTiers(input.providerId, input.model, input.effort).catch(failed),
    commands.routingSettings().catch(failed),
  ])
  if (mine !== seq) return
  maxAge.value = settings.status === 'ok' ? settings.data.settings.snapshotMaxAgeMinutes : null
  if (tiers.status === 'error') {
    view.value = null
    loadError.value = tiers.error
    return
  }
  loadError.value = null
  view.value = tiers.data
}

watch([providerId, model, effort], () => {
  clearTimeout(timer)
  refreshed.value = null
  refreshError.value = null
  timer = setTimeout(load, 300)
})
watch(
  () => props.version,
  () => void loadContext().then(load),
)
onMounted(() => void loadContext().then(load))

async function refresh(): Promise<void> {
  if (providerId.value === null) return
  busy.value = true
  refreshed.value = null
  refreshError.value = null
  const r = await commands.routingRefresh(providerId.value, model.value.trim(), effort.value).catch(failed)
  busy.value = false
  // Every Refresh that got past its checks is in the Refresh log, failed or not.
  emit('refreshed')
  if (r.status === 'error') {
    refreshError.value = r.error
    void load()
    return
  }
  refreshed.value = r.data
  loadError.value = null
  view.value = r.data.view
  // The model now has stored routing data, so it can be suggested.
  void commands
    .routingObserver()
    .catch(failed)
    .then((o) => (observer.value = o.status === 'ok' ? o.data : observer.value))
}

const openrouter = computed(() => inspectorProviders(providers.value))
const suggestions = computed(() => modelSuggestions(observer.value))
// The tiers couldn't load (a refused model, a provider that is gone): nothing below is ranked.
const blocked = computed(() => tiersBlocked(view.value, loadError.value))
const refreshButton = computed(() => {
  const state = refreshState(view.value, busy.value)
  return blocked.value ? { disabled: true, title: blocked.value } : state
})
const result = computed(() => view.value?.result ?? null)
const cards = computed(() => tierCards(view.value, maxAge.value))
const nitro = computed(() => nitroCard(view.value))
const rows = computed(() => candidateRows(result.value))
const registry = computed(() => registryLine(view.value))
const updated = computed(() => updatedText(view.value, now.value))
const hasInput = computed(() => providerId.value !== null && model.value.trim() !== '')
// The inputs are locked while a Refresh runs: its result belongs to the inputs it started with.
const lockTitle = computed(() => (busy.value ? 'Locked while a Refresh runs.' : undefined))
</script>

<template>
  <section class="inspect">
    <h3>Inspect</h3>
    <div class="inputs">
      <label>
        OpenRouter provider
        <select v-model="providerId" :disabled="busy || !openrouter.length" :title="lockTitle ?? (openrouter.length ? undefined : 'No OpenRouter provider yet.')">
          <option v-for="p in openrouter" :key="p.id" :value="p.id">{{ p.name }}</option>
        </select>
      </label>
      <label class="model">
        Model
        <input v-model="model" list="routing-models" :disabled="busy" :title="lockTitle" placeholder="deepseek/deepseek-v4.1-flash" maxlength="200" spellcheck="false" />
        <datalist id="routing-models">
          <option v-for="m in suggestions" :key="m" :value="m" />
        </datalist>
      </label>
      <label>
        Effort
        <select v-model="effort" :disabled="busy" :title="lockTitle">
          <option v-for="e in EFFORT_CHOICES" :key="e ?? 'none'" :value="e">{{ effortLabel(e) }}</option>
        </select>
      </label>
      <button type="button" class="refresh" :disabled="refreshButton.disabled" :title="refreshButton.title" @click="refresh">
        {{ busy ? 'Refreshing…' : 'Refresh' }}
      </button>
    </div>
    <p v-if="!openrouter.length" class="small">
      Routing needs an OpenRouter provider.
      <button type="button" class="link" @click="nav.openSettings('providers')">Add one in Settings → Providers</button>
    </p>

    <p v-if="loadError" class="small bad" role="alert">{{ loadError }}</p>
    <p v-else-if="!hasInput && openrouter.length" class="small">{{ NO_INPUT }}</p>
    <p v-if="refreshError" class="small bad" role="alert">{{ refreshError }}</p>
    <p v-else-if="busy" class="small" role="status">{{ REFRESHING }}</p>
    <p v-else-if="refreshed" class="small" role="status">{{ refreshText(refreshed) }}</p>
    <p v-for="n in refreshed?.notes ?? []" :key="n" class="small warn">{{ n }}</p>
    <p v-if="view?.problem" class="small warn">{{ view.problem }}</p>

    <template v-if="view">
      <p v-if="updated" class="small">{{ updated }}</p>
      <div class="cards">
        <article v-for="c in cards" :key="c.tier" class="card">
          <h4>{{ c.label }}</h4>
          <template v-if="c.primary">
            <p class="tag">{{ c.primary.tag }}</p>
            <p class="provider">{{ c.primary.provider }}</p>
            <dl class="facts">
              <dt>Speed</dt>
              <dd>{{ c.primary.speed }}</dd>
              <dt>Price</dt>
              <dd>{{ c.primary.price }}</dd>
              <dt>Latency</dt>
              <dd>{{ c.primary.latency }}</dd>
              <dt>Uptime</dt>
              <dd>{{ c.primary.uptime }}</dd>
            </dl>
            <p class="small">{{ c.fallbacks }}</p>
            <p class="small">{{ c.rationale }}</p>
          </template>
          <p v-else class="small">{{ c.empty }}</p>
          <p v-if="c.stale" class="small warn">{{ c.stale }}</p>
        </article>
        <article class="card">
          <h4>{{ nitro.label }}</h4>
          <p v-for="line in nitro.lines" :key="line" class="small">{{ line }}</p>
        </article>
        <article class="card">
          <h4>{{ DEFAULT_LABEL }}</h4>
          <p class="small">{{ DEFAULT_CARD }}</p>
        </article>
      </div>

      <ul v-if="result?.warnings.length" class="warnings">
        <li v-for="w in result.warnings" :key="w" class="small warn">{{ w }}</li>
      </ul>
      <p v-if="registry" class="small">{{ registry }}</p>

      <template v-if="rows.length">
        <h4 class="table-head">{{ candidateHeading(rows) }}</h4>
        <div class="scroll">
          <table class="all">
            <thead>
              <tr>
                <th>Endpoint</th>
                <th>Provider</th>
                <th>Speed</th>
                <th>Price</th>
                <th>Latency</th>
                <th>Uptime</th>
                <th>Why not</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="r in rows" :key="r.tag" :class="{ out: r.excluded }">
                <td class="mono">{{ r.tag }}</td>
                <td>{{ r.provider }}</td>
                <td>{{ r.speed }}</td>
                <td>{{ r.price }}</td>
                <td>{{ r.latency }}</td>
                <td>{{ r.uptime }}</td>
                <td class="why">{{ r.reasons }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </template>
    </template>
  </section>
</template>

<style scoped>
.inspect {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
h3 {
  margin: 0;
  font-size: 15px;
}
h4 {
  margin: 0;
  font-size: 13px;
}
.inputs {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 12px;
}
label {
  display: grid;
  gap: 5px;
  font-size: 12.5px;
  color: var(--text-2);
}
.model {
  flex: 1;
  min-width: 220px;
}
select,
input {
  padding: 7px 10px;
}
.refresh {
  padding: 7px 14px;
  font-weight: 600;
}
.link {
  padding: 0;
  border: none;
  background: none;
  color: var(--accent);
  font-size: 12px;
}
.small {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.5;
}
.warn {
  color: var(--status-needs);
}
.bad {
  color: var(--status-failed);
}
.mono {
  font-family: var(--font-mono);
}
.cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
  gap: 10px;
}
.card {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 12px 14px;
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  background: var(--surface);
}
.tag {
  margin: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text);
  overflow-wrap: anywhere;
}
.provider {
  margin: 0;
  font-size: 13px;
  color: var(--text-2);
}
.facts {
  display: grid;
  grid-template-columns: auto 1fr;
  gap: 2px 12px;
  margin: 4px 0;
  font-size: 12px;
}
.facts dt {
  color: var(--text-muted);
}
.facts dd {
  margin: 0;
  font-family: var(--font-mono);
}
.warnings {
  margin: 0;
  padding-left: 16px;
}
.table-head {
  margin-top: 6px;
  color: var(--text-2);
}
.scroll {
  overflow-x: auto;
}
.all {
  border-collapse: collapse;
  font-size: 12px;
}
.all th {
  padding: 3px 12px 3px 0;
  text-align: left;
  font-weight: 600;
  color: var(--text-muted);
}
.all td {
  padding: 3px 12px 3px 0;
  vertical-align: top;
}
.all tr.out {
  color: var(--text-muted);
}
.all .why {
  color: var(--text-muted);
}
</style>
