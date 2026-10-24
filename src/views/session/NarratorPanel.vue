<script setup lang="ts">
import { computed, ref } from 'vue'
import { commands, unwrap, type NarratorView, type SessionSummary, type NarratorAction } from '../../ipc'
import { useComposerStore } from '../../stores/composer'
import { useViewStore } from '../../stores/view'
const props = defineProps<{ session: SessionSummary; narrator: NarratorView | null; error: string }>()
const emit = defineEmits<{ refresh: [] }>()
const composer = useComposerStore(), view = useViewStore()
const busy = ref(false), notice = ref(''), actionError = ref('')
const summary = computed(() => props.narrator?.job?.summary)
const current = computed(() => props.narrator?.job?.turnId === props.narrator?.latestTurn)
const usableSummary = computed(() => !!summary.value && current.value && props.narrator?.enabled)
const showSummaryNow = computed(() => usableSummary.value && props.session.live && props.session.activity === 'ready')
const actions = computed(() => (props.narrator?.actions ?? []).filter(a => a.disposition === 'open' &&
  (a.restored || (usableSummary.value ? a.turnId === props.narrator?.job?.turnId : a.turnId === null))))
const hidden = computed(() => (props.narrator?.actions ?? []).filter(a => a.disposition !== 'open'))
const liveBlocker = computed(() => props.session.activity === 'awaiting-approval' ? 'The agent is waiting for approval. Use the active request below.' :
  props.session.activity === 'needs-answer' ? 'The agent needs your answer. Check its question below.' : '')
const liveNow = computed(() => props.session.activity === 'working' ? 'The agent is working. Follow its progress messages above.' :
  props.session.activity === 'failed' ? 'The last turn reported a failure.' : !props.session.live ? 'The session is stopped.' :
  props.session.activity === 'ready' ? 'The agent is ready for your next instruction.' : 'Check the latest agent message for its current status.')
async function change(a: NarratorAction, disposition: string) {
  busy.value = true; actionError.value = ''
  try { await unwrap(commands.narratorActionSet(props.session.id, a.id, disposition)); emit('refresh') }
  catch (e) { actionError.value = String(e) } finally { busy.value = false }
}
async function use(a: NarratorAction) {
  actionError.value = ''; notice.value = ''
  try {
    await composer.addSuggestion(props.session.id, a.suggestion.sends || a.suggestion.what)
    notice.value = 'Added to your draft below. Review it, then Send or Queue.'
  } catch (e) { actionError.value = String(e) }
}
async function summarize() {
  const turn = props.narrator?.latestTurn
  if (!turn) return
  busy.value = true; actionError.value = ''
  try { await unwrap(commands.narratorSummarize(props.session.id, turn)); emit('refresh') }
  catch (e) { actionError.value = String(e) } finally { busy.value = false }
}
async function toggle() {
  if (!props.session.projectId || !props.narrator) return
  busy.value = true; actionError.value = ''
  try { await unwrap(commands.narratorProjectSet(props.session.projectId, !props.narrator.enabled)); emit('refresh') }
  catch (e) { actionError.value = String(e) } finally { busy.value = false }
}
</script>

<template>
  <section class="narrator" aria-label="Session summary and next steps">
    <header><h2>At a glance</h2><button @click="view.openSettings('narrator')">Narrator settings</button></header>
    <p v-if="error || actionError" class="error" role="alert">{{ error || actionError }}</p>
    <template v-if="narrator">
      <p v-if="narrator.reason" class="muted">{{ narrator.reason }}</p>
      <p v-if="narrator.job?.status === 'queued' || narrator.job?.status === 'attempting'" class="muted" role="status">Preparing the completed-turn summary…</p>
      <p v-else-if="narrator.job?.note && narrator.job.note !== narrator.reason" :class="narrator.job.status === 'failed' ? 'error' : 'muted'">{{ narrator.job.note }}</p>
      <p v-if="showSummaryNow" class="muted">AI summary of turn {{ narrator.job?.turnId }} · {{ narrator.job?.model }}</p>
      <div class="glance">
        <div><h3>Now</h3><p>{{ showSummaryNow ? summary?.now : liveNow }}</p></div>
        <div><h3>Blocker</h3><p>{{ liveBlocker || (usableSummary ? summary?.blocker || 'No blocker reported in the last summary.' : 'Check the latest agent message and request cards for blockers.') }}</p></div>
        <div><h3>Up next</h3><p>{{ usableSummary ? summary?.next || 'Review the result.' : 'Review the latest result or prepare your next instruction.' }}</p></div>
      </div>
      <div v-if="summary && narrator.enabled && !showSummaryNow" class="summary">
        <h3>Last completed-turn summary <span>· Turn {{ narrator.job?.turnId }}{{ current ? '' : ' · Earlier turn' }}</span></h3>
        <p>{{ summary.now }}</p>
        <small>AI summary · {{ narrator.job?.model }}<template v-if="narrator.job?.finishedAtSecs"> · {{ new Date(narrator.job.finishedAtSecs * 1000).toLocaleTimeString() }}</template></small>
      </div>
      <button v-if="narrator.canSummarize" :disabled="busy" @click="summarize">Summarize latest turn</button>
      <div class="steps">
        <h3>Suggested next steps <span v-if="!usableSummary">· General suggestions</span></h3>
        <p v-if="!actions.length" class="muted">No open suggestions.</p>
        <article v-for="a in actions" :key="a.id">
          <strong>{{ a.suggestion.title }}</strong><p>{{ a.suggestion.what }}</p>
          <small v-if="a.suggestion.eta">{{ a.suggestion.eta }}</small>
          <details v-if="a.suggestion.sends"><summary>Preview prompt</summary><p>{{ a.suggestion.sends }}</p></details>
          <div class="buttons">
            <button :disabled="busy || session.locked || !session.live || !!composer.entry(session.id).state?.queueReason" @click="use(a)">Add to prompt</button>
            <button :disabled="busy" @click="change(a, 'done')">Done</button>
            <button :disabled="busy" @click="change(a, 'dismissed')">Dismiss</button>
          </div>
        </article>
      </div>
      <details v-if="hidden.length" class="hidden"><summary>Done and dismissed ({{ hidden.length }})</summary>
        <div v-for="a in hidden" :key="a.id"><span>{{ a.suggestion.title }} · {{ a.disposition }}</span><button :disabled="busy" @click="change(a, 'open')">Restore</button></div>
      </details>
      <p v-if="notice" role="status">{{ notice }}</p>
      <footer>
        <span>Narrator cost: {{ narrator.costs.reportedUsd === null ? 'Unavailable' : '$' + narrator.costs.reportedUsd.toFixed(5) + ' reported' }} · {{ narrator.costs.calls }} {{ narrator.costs.calls === 1 ? 'call' : 'calls' }}<span v-if="narrator.costs.unknownCostCalls"> · {{ narrator.costs.unknownCostCalls }} with cost unavailable</span></span>
        <button v-if="session.projectId" :disabled="busy" @click="toggle">{{ narrator.enabled ? 'Turn off for project' : 'Turn on for project' }}</button>
      </footer>
    </template>
  </section>
</template>

<style scoped>
.narrator { border: 1px solid var(--border); border-radius: 14px; padding: 20px; margin: 20px 0; background: var(--surface-raised); }
header, footer, .buttons, .hidden div { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; justify-content: space-between; }
h2 { font-size: 17px; margin: 0; } h3 { font-size: 13px; margin: 0 0 8px; } h3 span, small, footer, .muted { color: var(--text-muted); font-weight: 400; }
p { font-size: 13px; line-height: 1.6; white-space: pre-wrap; overflow-wrap: anywhere; margin: 7px 0; }
button { font-size: 12px; padding: 6px 10px; } .glance { display: grid; grid-template-columns: repeat(3,minmax(0,1fr)); gap: 20px; margin: 20px 0; }
.glance h3 { color: var(--accent); } .summary { border-top: 1px solid var(--border); padding: 16px 0; }
.steps { margin-top: 18px; } article { padding: 14px; margin: 10px 0; border: 1px solid var(--border); border-radius: 10px; }
article strong { font-size: 14px; } .buttons { justify-content: flex-start; margin-top: 12px; } details { font-size: 12px; margin: 10px 0; } summary { cursor: pointer; }
footer { font-size: 11px; margin-top: 18px; } .error { color: var(--status-failed); }
@media(max-width: 760px) { .glance { grid-template-columns: 1fr; gap: 12px; } }
</style>
