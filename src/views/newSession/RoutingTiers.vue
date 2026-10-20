<script setup lang="ts">
/**
 * Routing tiers for opencode on OpenRouter (D57, Chorus 1's routing): one compact row per tier with
 * the provider and its speed; the rest (price, latency, uptime, cache, fallbacks, why it won) in
 * the row's tooltip and the Details accordion. Refresh fetches the endpoints and checks the
 * account (both free), then may spend up to $0.05 testing prompt caching.
 */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import {
  commands,
  type CandidateExplanation,
  type Effort,
  type RefreshOutcome,
  type RoutingLaunchChoice,
  type TierSelection,
  type TiersView,
} from '../../ipc'

const props = defineProps<{ providerId: number; model: string; effort: Effort | null; modelValue: RoutingLaunchChoice | null }>()
const emit = defineEmits<{ 'update:modelValue': [choice: RoutingLaunchChoice | null] }>()

const view = ref<TiersView | null>(null)
const error = ref<string | null>(null)
const busy = ref(false)
const refreshed = ref<RefreshOutcome | null>(null)
const open = ref(false)
const showAll = ref(false)
// Once the user picks, a reload doesn't choose for them.
const touched = ref(false)
const now = ref(Date.now())
const ticker = setInterval(() => (now.value = Date.now()), 15_000)
onUnmounted(() => clearInterval(ticker))

let seq = 0
async function load(): Promise<void> {
  const mine = ++seq
  const r = await commands.routingTiers(props.providerId, props.model.trim(), props.effort).catch((e) => ({ status: 'error' as const, error: String(e) }))
  if (mine !== seq) return
  if (r.status === 'error') {
    view.value = null
    error.value = r.error
    emit('update:modelValue', null)
    return
  }
  error.value = null
  view.value = r.data
  if (!touched.value || props.modelValue === null) emit('update:modelValue', r.data.preselect)
}
let timer: ReturnType<typeof setTimeout> | undefined
watch(
  () => [props.providerId, props.model, props.effort],
  () => {
    clearTimeout(timer)
    refreshed.value = null
    timer = setTimeout(load, 300)
  },
)
onMounted(load)

async function refresh(): Promise<void> {
  busy.value = true
  error.value = null
  const r = await commands.routingRefresh(props.providerId, props.model.trim(), props.effort).catch((e) => ({ status: 'error' as const, error: String(e) }))
  busy.value = false
  if (r.status === 'error') {
    error.value = r.error
    void load()
    return
  }
  refreshed.value = r.data
  view.value = r.data.view
  if (!touched.value) emit('update:modelValue', r.data.view.preselect)
}

async function watchSpeeds(on: boolean): Promise<void> {
  const r = await commands.routingWatch(on ? props.providerId : null)
  if (r.status === 'error') error.value = r.error
  void load()
}

function choose(choice: RoutingLaunchChoice): void {
  touched.value = true
  emit('update:modelValue', choice)
}

const result = computed(() => view.value?.result ?? null)
const candidates = computed(() => new Map((result.value?.candidates ?? []).map((c) => [c.tag, c] as [string, CandidateExplanation])))
const eligible = computed(() => (result.value?.candidates ?? []).filter((c) => c.excludedBy.length === 0))
const excluded = computed(() => (result.value?.candidates ?? []).filter((c) => c.excludedBy.length > 0))
const updated = computed(() => {
  const at = result.value?.snapshotFetchedAtMs
  if (!at) return null
  const minutes = Math.floor((now.value - at) / 60_000)
  return minutes < 1 ? 'Updated just now' : `Updated ${minutes} min ago`
})

function tps(n: number | null | undefined): string {
  return n == null ? '—' : `${Math.round(n)} tok/s`
}
function price(n: number | null | undefined): string {
  return n == null ? '—' : `$${n >= 1 ? n.toFixed(2) : n.toPrecision(3)}/M`
}
function pct(n: number | null | undefined): string {
  return n == null ? '—' : `${n.toFixed(2)}%`
}
function secs(n: number | null | undefined): string {
  return n == null ? '—' : `${n.toFixed(2)} s`
}

// How each tier chose, in Chorus 1's words.
function why(tier: 'budget' | 'balanced' | 'fast'): string {
  const n = eligible.value.length
  if (tier === 'budget') {
    const above = eligible.value.filter((c) => !c.budgetFloorExcluded).length
    const floor = result.value?.budgetFloorTps
    return `First of ${above} endpoints at or above the ${floor == null ? '?' : floor.toFixed(1)} tok/s Budget floor, scored mostly on price.`
  }
  return tier === 'balanced' ? `First of ${n} eligible endpoints, scored equally on price and speed.` : `First of ${n} eligible endpoints, scored on speed only.`
}

function details(c: CandidateExplanation | undefined): string[] {
  if (!c) return []
  const lines = [
    `${c.tag} (${c.providerName}), ${c.quantization}`,
    `${price(c.cost?.blended)} blended · ${tps(c.tpsP50)} · ${c.effectiveTps == null ? '—' : `${c.effectiveTps.toFixed(1)} effective tok/s`}`,
    `${secs(c.latencyP50S)} latency · ${pct(c.uptime1d)} uptime`,
  ]
  const flags = [c.cost?.cacheCredit ? 'cache verified' : 'cache not verified', c.limitedHistory ? 'limited history' : `${c.observations} observations`]
  lines.push(flags.join(' · '))
  return lines
}

interface Row {
  choice: RoutingLaunchChoice
  label: string
  provider: string
  speed: string
  title: string
  disabled: boolean
}
const rows = computed<Row[]>(() => {
  const r = result.value
  const out: Row[] = []
  for (const tier of ['budget', 'balanced', 'fast'] as const) {
    const sel: TierSelection | null = r?.tiers[tier] ?? null
    const primary = sel ? candidates.value.get(sel.endpoints[0] ?? '') : undefined
    const label = tier[0]!.toUpperCase() + tier.slice(1)
    let title: string
    if (!r) title = 'Refresh to rank this model’s endpoints.'
    else if (!sel) title = `${label} has no eligible endpoints.`
    else
      title = [
        ...details(primary),
        sel.endpoints.length > 1 ? `Then: ${sel.endpoints.slice(1).join(', ')}` : 'No fallbacks',
        why(tier),
      ].join('\n')
    out.push({
      choice: tier,
      label,
      provider: primary ? `${primary.providerName}` : !r ? 'not ranked yet' : 'no eligible endpoint',
      speed: primary ? tps(primary.tpsP50) : '',
      title: r?.stale && sel ? `${title}\n\nOlder than 60 min: Refresh before launching with it.` : title,
      disabled: !sel || !!r?.stale,
    })
  }
  const likely = r?.nitro.likely ?? null
  out.push({
    choice: 'nitro',
    label: 'Nitro',
    provider: likely ? `likely ${likely.providerName}` : 'OpenRouter picks the fastest',
    speed: likely ? tps(likely.tpsP50) : '',
    title: [
      'Unfiltered provider routing: the :nitro model id, and OpenRouter sends each request to the fastest endpoint.',
      likely ? `Likely ${likely.providerName} (${likely.tag}) at ${tps(likely.tpsP50)}: an estimate; actual routing may differ.` : '',
      r?.nitro.likelyFailsRules.length ? `That endpoint fails: ${r.nitro.likelyFailsRules.join('; ')}.` : '',
      'Requests may be billed at a provider’s priority-tier price. Your account’s guardrails still apply.',
    ]
      .filter(Boolean)
      .join('\n'),
    disabled: false,
  })
  out.push({
    choice: 'default',
    label: 'OpenRouter default',
    provider: 'OpenRouter picks per request',
    speed: '',
    title: 'Chorus sends no routing: OpenRouter picks the provider for each request, as before. The data-collection setting is not sent.',
    disabled: false,
  })
  return out
})
const chosen = computed(() => (props.modelValue === 'budget' || props.modelValue === 'balanced' || props.modelValue === 'fast' ? result.value?.tiers[props.modelValue] ?? null : null))
const refreshTitle =
  'Fetches the endpoint list and checks what your account allows (both free), then may spend up to about $0.05 of OpenRouter credit testing prompt caching.'
const spentText = computed(() => {
  const r = refreshed.value
  if (!r) return null
  if (!r.probed.length) return 'Refreshed. No cache test needed: every eligible endpoint was tested in the last 14 days.'
  return `Refreshed. Tested prompt caching on ${r.probed.length} endpoint${r.probed.length === 1 ? '' : 's'} for $${(r.spentUsd ?? 0).toFixed(4)} (at most $${(r.estimateUsd ?? 0).toFixed(4)}).`
})
</script>

<template>
  <section class="routing">
    <div class="head">
      <span class="label">Routing</span>
      <span v-if="updated" class="small" :class="{ warn: result?.stale }">{{ updated }}{{ result?.stale ? ' · older than 60 min' : '' }}</span>
      <span class="spacer" />
      <button
        type="button"
        class="refresh"
        :disabled="busy || !!view?.problem || (view?.retryInSecs ?? 0) > 0"
        :title="view?.problem ?? ((view?.retryInSecs ?? 0) > 0 ? `Refreshed less than a minute ago: again in ${view?.retryInSecs} s` : refreshTitle)"
        @click="refresh"
      >
        {{ busy ? 'Refreshing…' : 'Refresh' }}
      </button>
    </div>
    <p v-if="error" class="small bad">{{ error }}</p>
    <p v-else-if="view?.problem && !result" class="small warn">{{ view.problem }}</p>
    <p v-else-if="busy" class="small">Fetching endpoints, checking the account, then testing prompt caching (at most $0.05)…</p>
    <p v-else-if="spentText" class="small">{{ spentText }}</p>
    <p v-for="n in refreshed?.notes ?? []" :key="n" class="small warn">{{ n }}</p>
    <p v-if="view?.preselectHint && !touched" class="small">{{ view.preselectHint }}</p>

    <div class="rows" role="radiogroup" aria-label="Routing tier">
      <button
        v-for="r in rows"
        :key="r.choice"
        type="button"
        role="radio"
        class="row"
        :class="{ on: modelValue === r.choice }"
        :aria-checked="modelValue === r.choice"
        :disabled="r.disabled"
        :title="r.title"
        @click="choose(r.choice)"
      >
        <span class="dot" />
        <span class="tier">{{ r.label }}</span>
        <span class="provider">{{ r.provider }}</span>
        <span class="speed">{{ r.speed }}</span>
      </button>
    </div>

    <button v-if="result" type="button" class="quiet disclosure" :aria-expanded="open" @click="open = !open">
      <span class="caret" :class="{ open }">›</span> Details
      <span v-if="result.warnings.length" class="small warn">· {{ result.warnings.length }} warning{{ result.warnings.length === 1 ? '' : 's' }}</span>
    </button>
    <div v-if="open && result" class="details">
      <template v-if="chosen">
        <p class="small strong">{{ chosen.rationale }}</p>
        <p v-for="(t, i) in chosen.endpoints" :key="t" class="small mono">
          {{ i === 0 ? 'First' : `Then ${i}` }}: {{ t }} · {{ tps(candidates.get(t)?.tpsP50) }} · {{ price(candidates.get(t)?.cost?.blended) }}
        </p>
      </template>
      <ul v-if="result.warnings.length" class="warnings">
        <li v-for="w in result.warnings" :key="w" class="small warn">{{ w }}</li>
      </ul>
      <p v-if="view?.registry" class="small">
        {{ view.registry.source === 'bundled' ? 'Chorus knows this model' : 'Derived from its endpoints' }}: at least
        {{ view.registry.nativePrecision ?? 'any precision' }}, {{ view.registry.minContext.toLocaleString() }} tokens of context.
      </p>
      <label class="small watch" :title="view?.problem ?? 'Every 30 minutes, the free endpoint list for each refreshed model, with this provider’s key. No account check, no cache test.'">
        <input type="checkbox" :checked="view?.watching" :disabled="!!view?.problem" @change="watchSpeeds(($event.target as HTMLInputElement).checked)" />
        Keep speeds fresh every 30 minutes with this provider's key (free)
      </label>
      <button type="button" class="quiet disclosure" :aria-expanded="showAll" @click="showAll = !showAll">
        <span class="caret" :class="{ open: showAll }">›</span>
        All providers ({{ eligible.length }} eligible · {{ excluded.length }} excluded)
      </button>
      <table v-if="showAll" class="all">
        <tbody>
          <tr v-for="c in result.candidates" :key="c.tag" :class="{ out: c.excludedBy.length }" :title="details(c).join('\n')">
            <td class="mono">{{ c.tag }}</td>
            <td>{{ tps(c.tpsP50) }}</td>
            <td>{{ price(c.cost?.blended) }}</td>
            <td class="why">{{ c.excludedBy.join('; ') || (c.budgetFloorExcluded ? 'below the Budget floor' : '') }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>

<style scoped>
.routing {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.head {
  display: flex;
  align-items: center;
  gap: 10px;
}
.label {
  color: var(--text-2);
  font-size: 13px;
  font-weight: 600;
}
.spacer {
  flex: 1;
}
.small {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
}
.strong {
  color: var(--text-2);
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
.refresh {
  padding: 3px 12px;
  font-size: 12px;
  font-weight: 600;
}
.rows {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.row {
  display: grid;
  grid-template-columns: 14px 128px minmax(0, 1fr) auto;
  align-items: center;
  gap: 10px;
  padding: 6px 10px;
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  color: var(--text-2);
  font-size: 13px;
  text-align: left;
}
.row:hover:not(:disabled) {
  background: var(--surface);
  border-color: transparent;
}
.row.on {
  background: var(--accent-soft);
  color: var(--text);
}
.dot {
  width: 14px;
  height: 14px;
  border-radius: 50%;
  border: 1.5px solid var(--border-strong);
  box-sizing: border-box;
}
.row.on .dot {
  border: 4px solid var(--accent);
}
.tier {
  font-weight: 600;
}
.provider {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.speed {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-muted);
}
.row.on .speed {
  color: var(--text-2);
}
.disclosure {
  display: flex;
  align-items: baseline;
  gap: 6px;
  padding: 2px 0;
  font-size: 12.5px;
}
.caret {
  display: inline-block;
  transition: transform 120ms ease;
}
.caret.open {
  transform: rotate(90deg);
}
.details {
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 2px 0 2px 16px;
}
.warnings {
  margin: 0;
  padding-left: 16px;
}
.watch {
  display: flex;
  align-items: center;
  gap: 7px;
}
.all {
  border-collapse: collapse;
  font-size: 12px;
}
.all td {
  padding: 3px 8px 3px 0;
  vertical-align: top;
}
.all tr.out {
  color: var(--text-muted);
}
.all .why {
  color: var(--text-muted);
}
</style>
