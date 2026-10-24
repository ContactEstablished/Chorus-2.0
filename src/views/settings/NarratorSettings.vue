<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { commands, unwrap, type NarratorSettings, type ProviderSummary, type CatalogEntry } from '../../ipc'
import { useViewStore } from '../../stores/view'
const view = useViewStore()
const settings = ref<NarratorSettings | null>(null), providers = ref<ProviderSummary[]>([]), catalog = ref<CatalogEntry[]>([])
const providerId = ref<number | null>(null), model = ref(''), error = ref(''), notice = ref(''), busy = ref(false)
const search = ref('')
const modelCatalog = ref<HTMLDetailsElement | null>(null)
function selectModel(id: string) {
  model.value = id
  if (modelCatalog.value) {
    modelCatalog.value.open = false
    modelCatalog.value.querySelector('summary')?.focus({ preventScroll: true })
  }
}
const models = computed(() => catalog.value.filter(c => !c.missing && `${c.model.id} ${c.model.name}`.toLowerCase().includes(search.value.toLowerCase()))
  .sort((a, b) => (a.model.promptPerMtok ?? Infinity) - (b.model.promptPerMtok ?? Infinity)).slice(0, 100))
const selected = computed(() => catalog.value.find(c => c.model.id === model.value)?.model)
async function loadCatalog() {
  catalog.value = []
  if (providerId.value) catalog.value = await unwrap(commands.providerCatalog(providerId.value))
}
async function load() {
  try {
    const [s, p] = await Promise.all([unwrap(commands.narratorSettings()), unwrap(commands.providersList())])
    settings.value = s; providers.value = p.filter(p => p.kind === 'openrouter')
    providerId.value = s.config.providerId; model.value = s.config.model
    await loadCatalog()
  } catch (e) { error.value = String(e) }
}
async function providerChanged() {
  error.value = ''; model.value = ''
  try { await loadCatalog() } catch (e) { error.value = String(e) }
}
async function refreshCatalog() {
  if (!providerId.value) return
  busy.value = true; error.value = ''
  try { await unwrap(commands.providerRefreshCatalog(providerId.value)); await loadCatalog() }
  catch (e) { error.value = String(e) } finally { busy.value = false }
}
async function save() {
  busy.value = true; error.value = ''; notice.value = ''
  try {
    await unwrap(commands.narratorConfigure({ providerId: providerId.value, model: model.value.trim() }))
    settings.value = await unwrap(commands.narratorSettings())
    notice.value = 'Saved. New completed turns use these settings. Older turns are summarized only when you request it.'
  } catch (e) { error.value = String(e) } finally { busy.value = false }
}
async function toggle(id: number, enabled: boolean) {
  busy.value = true; error.value = ''
  try { await unwrap(commands.narratorProjectSet(id, enabled)); settings.value = await unwrap(commands.narratorSettings()) }
  catch (e) { error.value = String(e) } finally { busy.value = false }
}
onMounted(load)
</script>
<template>
  <section class="narrator-settings">
    <h2>Narrator</h2>
    <p>Summaries and suggested next steps in Friendly, using a small OpenRouter call after each completed turn.</p>
    <p class="muted">Only scrubbed excerpts of the prompt, result, plan, tool summaries and change counts are sent. Image contents and full files are not sent. Suggestions are added to your draft for review.</p>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-if="notice" role="status">{{ notice }}</p>
    <p v-if="settings?.reason" class="muted">{{ settings.reason }}</p>
    <form @submit.prevent="save">
      <label>OpenRouter provider<select v-model="providerId" :disabled="busy" @change="providerChanged"><option :value="null">Not configured</option><option v-for="p in providers" :key="p.id" :value="p.id">{{ p.name }}{{ p.credentialId ? '' : ' · API key needed' }}</option></select></label>
      <button type="button" @click="view.openSettings('providers')">Manage providers and keys</button>
      <label>Model ID<input v-model="model" :disabled="busy || !providerId" placeholder="Select a model below or enter its ID" maxlength="200" /></label>
      <details v-if="providerId" ref="modelCatalog"><summary>Choose from model catalog</summary>
        <div class="catalog-controls"><input v-model="search" placeholder="Search models…" aria-label="Search narrator models" /><button type="button" :disabled="busy" @click="refreshCatalog">Refresh models</button></div>
        <p class="muted">Sorted by input price. Choose a model with structured-output support; unsupported models show a visible error.</p>
        <div class="models"><button v-for="c in models" :key="c.model.id" type="button" :disabled="busy" @click="selectModel(c.model.id)">{{ c.model.name }} <small>{{ c.model.id }} · ${{ c.model.promptPerMtok ?? '?' }} / ${{ c.model.completionPerMtok ?? '?' }} per million input / output tokens</small></button></div>
        <p v-if="!models.length">No matching models. Refresh the catalog or enter a model ID.</p>
      </details>
      <p v-if="selected" class="muted">Catalog price: ${{ selected.promptPerMtok ?? '?' }} input / ${{ selected.completionPerMtok ?? '?' }} output per million tokens. Actual reported cost appears after each call.</p>
      <button type="submit" class="primary" :disabled="busy">Save narrator settings</button>
    </form>
    <template v-if="settings">
      <h3>Project switches</h3><p class="muted">On by default when configured. Turning a project off stops future calls; a call already sent may still finish. Sessions without a project use the narrator when configured.</p>
      <label v-for="p in settings.projects" :key="p.id" class="project"><input type="checkbox" :checked="p.enabled" :disabled="busy" @change="toggle(p.id, !p.enabled)" />{{ p.name }}</label>
      <p v-if="!settings.projects.length" class="muted">Add a project in Settings → Projects to control narration for its sessions.</p>
      <h3>Narrator usage</h3><p>{{ settings.costs.reportedUsd === null ? 'Cost unavailable' : '$' + settings.costs.reportedUsd.toFixed(5) + ' reported' }} across {{ settings.costs.calls }} calls.</p>
      <p v-if="settings.costs.unknownCostCalls" class="muted">{{ settings.costs.unknownCostCalls }} calls have unavailable costs and are excluded from that total.</p>
      <p class="muted">This is narrator usage only. Agent session costs are separate. Failed or interrupted attempts are never retried automatically.</p>
    </template>
  </section>
</template>
<style scoped>
h2 { margin-top: 0; } h3 { margin-top: 28px; font-size: 15px; } p { font-size: 13px; line-height: 1.6; }
.muted { color: var(--text-muted); } .error { color: var(--status-failed); } form { display: grid; gap: 14px; margin: 22px 0; }
label { display: grid; gap: 7px; font-size: 13px; } select, input { padding: 9px 12px; width: 100%; } button { width: fit-content; padding: 8px 12px; }
.catalog-controls { display: flex; gap: 10px; margin-top: 12px; } .models { max-height: 260px; overflow: auto; display: grid; gap: 5px; }
.models button { width: 100%; text-align: left; } small { display: block; color: var(--text-muted); margin-top: 5px; } summary { cursor: pointer; font-size: 13px; }
.project { display: flex; align-items: center; margin: 12px 0; } .project input { width: auto; }
</style>
