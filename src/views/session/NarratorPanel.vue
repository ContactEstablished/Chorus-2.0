<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { commands, unwrap, type Panel, type Disposition } from '../../ipc'

const props = defineProps<{ sessionId: number }>()
const emit = defineEmits<{ draft: [text: string] }>()
const panel = ref<Panel | null>(null)
const error = ref<string | null>(null)
const busy = ref(false)
const open = computed(() => panel.value?.actions.filter((a) => a.status === 'open') ?? [])
const resolved = computed(() => panel.value?.actions.filter((a) => a.status !== 'open') ?? [])
let generation = 0
async function load(): Promise<void> {
  const current = ++generation
  const id = props.sessionId
  try {
    const value = await unwrap(commands.narratorPanel(id))
    if (generation === current && id === props.sessionId) { panel.value = value; error.value = null }
  } catch (e) { if (generation === current) error.value = String(e) }
}
watch(() => props.sessionId, () => { panel.value = null; void load() }, { immediate: true })
const timer = window.setInterval(() => void load(), 2000)
onBeforeUnmount(() => { generation++; window.clearInterval(timer) })
async function resolve(id: number, disposition: Disposition): Promise<void> {
  busy.value = true
  try { await unwrap(commands.narratorResolve(props.sessionId, id, disposition)); await load() }
  catch (e) { error.value = String(e) }
  finally { busy.value = false }
}
</script>

<template>
  <section class="narrator" aria-label="Narrator and Next steps">
    <p v-if="error" class="notice" role="alert">{{ error }}</p>
    <template v-if="panel">
      <p v-if="panel.status === 'running' || panel.status === 'pending'" class="notice" role="status">Summary updating…</p>
      <p v-if="panel.reason" class="notice">{{ panel.reason }}</p>
      <p v-if="panel.turnId && panel.status === 'ready'" class="caption">Summary of completed turn {{ panel.turnId }}</p>
      <p v-if="panel.status === 'ready' && panel.summary.is_question" class="notice">The narrator thinks this completed turn asks you a question. Review the final message before answering.</p>
      <dl>
        <dt>Now</dt><dd>{{ panel.summary.now }}</dd>
        <dt>Blocker</dt><dd>{{ panel.summary.blocker ?? (panel.status === 'ready' ? 'No blocker reported in this summary.' : 'Unavailable without a completed summary.') }}</dd>
        <dt>Up next</dt><dd>{{ panel.summary.next }}</dd>
      </dl>
      <h3>Next steps</h3>
      <p class="caption">Choose a draft, review it in the composer, then press Send. Suggestions never answer approval cards.</p>
      <p v-if="!open.length" class="caption">No open suggestions. Write your own instruction in the composer.</p>
      <article v-for="action in open" :key="action.id" class="step">
        <h4>{{ action.suggestion.title }}</h4>
        <p>{{ action.suggestion.what }}</p>
        <details><summary>What it sends</summary><pre>{{ action.suggestion.sends }}</pre></details>
        <small>{{ action.suggestion.eta }}</small>
        <div class="buttons">
          <button @click="emit('draft', action.suggestion.sends)">Use draft</button>
          <button class="quiet" :disabled="busy" @click="resolve(action.id, 'done')">Done</button>
          <button class="quiet" :disabled="busy" @click="resolve(action.id, 'dismissed')">Dismiss</button>
        </div>
      </article>
      <details v-if="resolved.length" class="resolved">
        <summary>Done / dismissed — won't be suggested again ({{ resolved.length }})</summary>
        <p v-for="action in resolved" :key="action.id">{{ action.suggestion.title }} <small>· {{ action.status }}</small></p>
      </details>
      <p class="caption">Narrator recorded cost: ${{ (panel.knownCostUsd ?? 0).toFixed(6) }}<template v-if="panel.unknownCostCalls"> + {{ panel.unknownCostCalls }} call(s) with unknown cost</template>.</p>
    </template>
  </section>
</template>

<style scoped>
.narrator { display: grid; gap: 12px; font-size: 13px; }
dl, dd, h3, h4, p { margin: 0; }
dl { display: grid; gap: 4px; }
dt { font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: var(--text-muted); margin-top: 8px; }
dd, p { overflow-wrap: anywhere; line-height: 1.45; }
.notice { color: var(--status-needs); background: var(--surface); padding: 10px; border-radius: 8px; }
.caption, small { color: var(--text-muted); font-size: 11px; line-height: 1.45; }
.step { display: grid; gap: 9px; padding: 12px; border: 1px solid var(--border); border-radius: 10px; background: var(--surface); }
.step p { color: var(--text-2); }
summary { cursor: pointer; color: var(--text-2); font-size: 12px; }
pre { white-space: pre-wrap; overflow-wrap: anywhere; font: 11px/1.5 var(--font-mono); }
.buttons { display: flex; flex-wrap: wrap; gap: 4px; }
button { font-size: 12px; padding: 4px 8px; }
.resolved p { margin: 8px 0; }
</style>
