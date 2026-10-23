<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { commands, unwrap, type SessionHistory, type SessionSummary, type PendingReply, type SessionProgress } from '../../ipc'
import ReplyCard from './ReplyCard.vue'
import FriendlyPlan from './FriendlyPlan.vue'
import FriendlyMessages from './FriendlyMessages.vue'

const props = defineProps<{ session: SessionSummary }>()
const emit = defineEmits<{ raw: [] }>()
const history = ref<SessionHistory | null>(null)
const pending = ref<PendingReply[]>([])
const progress = ref<SessionProgress | null>(null)
const progressError = ref('')
const error = ref('')
let generation = 0
let timer: ReturnType<typeof setTimeout> | undefined
let disposed = false
async function load(): Promise<void> {
  clearTimeout(timer)
  const current = ++generation
  const id = props.session.id
  try {
    const [h, p, updates] = await Promise.all([
      unwrap(commands.sessionHistory(id, 100)), unwrap(commands.sessionPendingReplies(id)),
      unwrap(commands.sessionProgress(id)).then(data => ({ data, error: '' }), e => ({ data: null, error: String(e) })),
    ])
    if (disposed || current !== generation) return
    history.value = h
    pending.value = p
    progress.value = updates.data
    progressError.value = updates.error
    error.value = ''
  } catch (e) {
    if (current === generation) { error.value = String(e); pending.value = [] }
  } finally {
    if (!disposed && current === generation) timer = setTimeout(() => void load(), 1500)
  }
}
watch(() => props.session.id, () => { history.value = null; pending.value = []; progress.value = null; progressError.value = ''; void load() }, { immediate: true })
onBeforeUnmount(() => { disposed = true; generation++; clearTimeout(timer) })
</script>

<template>
  <div class="friendly scroll">
    <div class="intro">
      <h2>Session timeline</h2>
      <p>Recorded messages, tools and requests from this running session. Raw shows the live terminal.</p>
    </div>
    <p v-if="error" class="error" role="alert">History could not refresh: {{ error }}. <button @click="emit('raw')">Open Raw</button></p>
    <div v-if="pending.length" class="pending">
      <ReplyCard v-for="p in pending" :key="`${p.runId}-${p.askId}`" :session-id="session.id" :pending="p" @refresh="load" @raw="emit('raw')" />
    </div>
    <p class="limitation">The instruction queue is not connected yet. Use Raw for new instructions. Narrated summaries and Next steps are not available yet.</p>
    <p v-if="progressError" class="error" role="alert">Progress could not refresh: {{ progressError }}. Check Raw for the current plan and messages.</p>
    <template v-if="progress">
      <p class="limitation">{{ progress.note }}</p>
      <p v-if="progress.error" class="error" role="alert">{{ progress.error }}</p>
      <FriendlyPlan :plan="progress.plan" :current-run-id="progress.currentRunId" />
      <FriendlyMessages :messages="progress.messages" :turn-id="null" />
    </template>
    <p v-if="history && !history.turns.length" class="empty">{{ session.hasEventChannel ? 'Waiting for the first recorded prompt.' : 'This agent has no structured history channel. Use Raw.' }}</p>
    <ol v-if="history" class="timeline">
      <li v-for="turn in history.turns" :key="turn.id" class="turn">
        <div class="meta"><time>{{ new Date(turn.startedAtSecs * 1000).toLocaleString() }}</time><span>{{ turn.endReason ?? 'In progress' }}</span></div>
        <article class="message user"><h3>You <small>{{ turn.promptSource }}</small></h3><p>{{ turn.prompt ?? 'Prompt was not observed.' }}</p></article>
        <FriendlyMessages v-if="progress" :messages="progress.messages" :turn-id="turn.id" :final-message="turn.finalMessage" />
        <details v-if="turn.tools.length" class="tools">
          <summary>{{ turn.tools.length }} tool {{ turn.tools.length === 1 ? 'call' : 'calls' }}</summary>
          <ul><li v-for="(tool, i) in turn.tools" :key="i"><strong>{{ tool.tool }}</strong> · {{ tool.failed ? 'Failed' : tool.finished ? 'Finished' : 'Started' }}<code v-if="tool.target">{{ tool.target }}</code></li></ul>
        </details>
        <article v-for="ask in turn.asks" :key="ask.id" class="ask">
          <h3>{{ ask.kind === 'approval' ? 'Approval' : 'Question' }}</h3>
          <p>{{ ask.subject }}</p>
          <p v-if="ask.replyAction?.kind === 'approval'">You requested: {{ ask.replyAction.decision === 'allow' ? 'allow once' : 'deny' }}<template v-if="ask.replyAction.message"> — {{ ask.replyAction.message }}</template></p>
          <p v-else-if="ask.replyAction?.kind === 'answer'">You sent: {{ ask.replyAction.answers.join(' · ') }}</p>
          <p v-else-if="ask.replyAction?.kind === 'release'">Returned to Raw without a decision.</p>
          <p>{{ ask.answer ?? (ask.replyAttempted ? 'Reply attempted; agent outcome not yet observed.' : 'No answer observed. Use the active card above, or Raw.') }}</p>
          <details v-if="ask.details"><summary>Full request</summary><pre>{{ ask.details.kind === 'approval' ? ask.details.input : JSON.stringify(ask.details.questions, null, 2) }}</pre></details>
        </article>
        <article v-if="turn.finalMessage" class="message agent"><h3>Agent</h3><p>{{ turn.finalMessage }}</p></article>
      </li>
    </ol>
    <p v-if="history && history.total > history.turns.length" class="limitation">Showing the latest {{ history.turns.length }} of {{ history.total }} turns.</p>
  </div>
</template>

<style scoped>
.friendly { padding: 24px; min-height: 0; }
h2 { margin: 0; font-size: 19px; }
.intro p, .empty, .limitation { font-size: 13px; color: var(--text-muted); line-height: 1.6; }
.limitation { padding: 12px; border: 1px dashed var(--border-strong); border-radius: 10px; }
.pending { display: grid; gap: 14px; margin: 18px 0; }
.timeline { list-style: none; margin: 20px 0; padding: 0; }
.turn { display: grid; gap: 12px; margin-bottom: 28px; }
.meta { display: flex; justify-content: space-between; gap: 12px; color: var(--text-muted); font-size: 12px; }
.message, .ask { padding: 16px 18px; background: var(--surface-raised); border: 1px solid var(--border); border-radius: 14px; }
.user { background: var(--surface); }
h3 { margin: 0 0 8px; font-size: 13px; }
h3 small { color: var(--text-muted); font-weight: 400; margin-left: 8px; }
article p { white-space: pre-wrap; overflow-wrap: anywhere; margin: 6px 0; font-size: 14px; line-height: 1.6; }
.tools { border-left: 2px solid var(--border-strong); padding: 8px 14px; font-size: 12px; color: var(--text-2); }
summary { cursor: pointer; }
.tools li { margin: 8px 0; }
code { display: block; overflow-wrap: anywhere; margin: 4px 0; }
pre { white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; max-height: 320px; overflow: auto; }
.error { color: var(--status-failed); }
</style>
