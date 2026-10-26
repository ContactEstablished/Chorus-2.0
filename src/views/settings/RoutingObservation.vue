<script setup lang="ts">
/**
 * Settings → Routing, part 4: the background observation. Choosing a provider is the consent to
 * record endpoint numbers every 30 minutes with its key (`routing_watch`); no provider is off. It
 * shows the last check and the next one, and each model the observer follows, with Stop observing
 * and Observe again (the model's data stays either way). The status lives in memory, so after a
 * restart it reports no check yet. Reloads when `version` changes, and when the next check has come.
 */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { commands, type ProviderSummary } from '../../ipc'
import type { ObserverView } from '../../ipc/bindings'
import { useViewStore } from '../../stores/view'
import { NO_MODELS, OFF_LABEL, chooseProvider, modelRows, observerLines, observerProviders, overdue, setObserved } from './routingObservation'

const props = defineProps<{ version: number }>()
const nav = useViewStore()

const providers = ref<ProviderSummary[]>([])
const view = ref<ObserverView | null>(null)
// What the select shows; it goes back to the saved provider when a choice is refused.
const choice = ref<number | null>(null)
const loadError = ref<string | null>(null)
const actionError = ref<string | null>(null)
const busy = ref(false)
const now = ref(Date.now())

const failed = (e: unknown) => ({ status: 'error' as const, error: String(e) })

function adopt(v: ObserverView): void {
  view.value = v
  choice.value = v.providerId
  loadError.value = null
}

async function load(): Promise<void> {
  const [p, o] = await Promise.all([commands.providersList().catch(failed), commands.routingObserver().catch(failed)])
  providers.value = p.status === 'ok' ? p.data : []
  if (o.status === 'error') loadError.value = o.error
  else adopt(o.data)
}

// The clock moves the relative times; the status is read again once the next check is due, so a
// check that ends while this is open shows without leaving the section.
let reloading = false
const ticker = setInterval(() => {
  now.value = Date.now()
  if (busy.value || reloading || !overdue(view.value, now.value)) return
  reloading = true
  void load().finally(() => (reloading = false))
}, 15_000)
onUnmounted(() => clearInterval(ticker))

onMounted(() => void load())
watch(
  () => props.version,
  () => void load(),
)

async function pick(): Promise<void> {
  busy.value = true
  actionError.value = null
  const r = await chooseProvider(choice.value, commands.routingWatch, commands.routingObserver)
  busy.value = false
  if (!r.ok) {
    actionError.value = r.reason
    choice.value = view.value?.providerId ?? null
    return
  }
  adopt(r.view)
}

async function observe(model: string, again: boolean): Promise<void> {
  busy.value = true
  actionError.value = null
  const r = await setObserved(model, again, commands.routingObserveModel)
  busy.value = false
  if (!r.ok) {
    actionError.value = r.reason
    return
  }
  adopt(r.view)
}

const choices = computed(() => observerProviders(providers.value))
const lines = computed(() => (view.value ? observerLines(view.value, now.value) : []))
const rows = computed(() => (view.value ? modelRows(view.value, now.value) : []))
const selectTitle = computed(() => (busy.value ? 'Working…' : view.value ? undefined : loadError.value ? 'The status couldn’t be loaded.' : 'Loading…'))
</script>

<template>
  <section class="observation">
    <h3>Background observation</h3>
    <label>
      Provider whose key it uses
      <select v-model="choice" :disabled="!view || busy" :title="selectTitle" @change="pick">
        <option :value="null">{{ OFF_LABEL }}</option>
        <option v-for="c in choices" :key="c.id" :value="c.id" :disabled="c.disabled" :title="c.title">{{ c.label }}</option>
      </select>
    </label>
    <p v-if="!choices.length" class="small">
      Routing needs an OpenRouter provider.
      <button type="button" class="link" @click="nav.openSettings('providers')">Add one in Settings → Providers</button>
    </p>

    <p v-if="loadError" class="small bad" role="alert">
      The observation status couldn’t be loaded: {{ loadError }}
      <button type="button" class="link" @click="load">Try again</button>
    </p>
    <p v-if="actionError" class="small bad" role="alert">{{ actionError }}</p>
    <p v-for="l in lines" :key="l.text" class="small" :class="{ warn: l.tone === 'warn' }">{{ l.text }}</p>

    <template v-if="view">
      <h4>Models it follows</h4>
      <p v-if="view.stoppedProblem" class="small warn">{{ view.stoppedProblem }}</p>
      <p v-if="!rows.length" class="small">{{ NO_MODELS }}</p>
      <ul v-else class="models">
        <li v-for="r in rows" :key="r.model" :class="{ stopped: r.stopped }">
          <span class="slug">{{ r.model }}</span>
          <span class="mark">{{ r.mark }}</span>
          <span class="small">{{ r.status }}</span>
          <button type="button" :disabled="busy" :title="busy ? 'Working…' : undefined" @click="observe(r.model, r.observe)">{{ r.action }}</button>
        </li>
      </ul>
    </template>
  </section>
</template>

<style scoped>
.observation {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
h3 {
  margin: 0;
  font-size: 15px;
}
h4 {
  margin: 6px 0 0;
  font-size: 13px;
  color: var(--text-2);
}
label {
  display: grid;
  gap: 5px;
  width: fit-content;
  min-width: 220px;
  font-size: 12.5px;
  color: var(--text-2);
}
select {
  padding: 7px 10px;
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
  max-width: 70ch;
}
.warn {
  color: var(--status-needs);
}
.bad {
  color: var(--status-failed);
}
.models {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 0;
  padding: 0;
  list-style: none;
}
.models li {
  display: grid;
  grid-template-columns: minmax(200px, 1fr) auto minmax(180px, auto) auto;
  align-items: center;
  gap: 4px 14px;
  font-size: 12.5px;
}
.models li.stopped .slug {
  color: var(--text-muted);
}
.slug {
  font-family: var(--font-mono);
  overflow-wrap: anywhere;
}
.mark {
  color: var(--status-needs);
  font-size: 12px;
  font-weight: 600;
}
.models button {
  padding: 5px 12px;
}
</style>
