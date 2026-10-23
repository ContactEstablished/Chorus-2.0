<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { commands, unwrap, type SessionHistory, type SessionSummary, type PendingReply } from '../../ipc'
import ReplyCard from './ReplyCard.vue'
import SessionComposer from './SessionComposer.vue'

const props = defineProps<{ session: SessionSummary }>()
const emit = defineEmits<{ raw: [] }>()
const history = ref<SessionHistory | null>(null)
const pending = ref<PendingReply[]>([])
const error = ref('')
let generation = 0
let timer: ReturnType<typeof setTimeout> | undefined
let disposed = false
async function load(): Promise<void> {
  clearTimeout(timer)
  const current = ++generation
  const id = props.session.id
  try {
    const [h, p] = await Promise.all([unwrap(commands.sessionHistory(id, 100)), unwrap(commands.sessionPendingReplies(id))])
    if (disposed || current !== generation) return
    history.value = h
    pending.value = p
    error.value = ''
  } catch (e) {
    if (current === generation) { error.value = String(e); pending.value = [] }
  } finally {
    if (!disposed && current === generation) timer = setTimeout(() => void load(), 1500)
  }
}
watch(() => props.session.id, () => { history.value = null; pending.value = []; void load() }, { immediate: true })
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
    <p class="limitation">Plan capture is not connected yet. Narrated summaries and Next steps are not available yet.</p>
    <p v-if="history && !history.turns.length" class="empty">{{ session.hasEventChannel ? 'Waiting for the first recorded prompt.' : 'This agent has no structured history channel. Use Raw.' }}</p>
    <ol v-if="history" class="timeline">
      <li v-for="turn in history.turns" :key="turn.id" class="turn">
        <div class="meta"><time>{{ new Date(turn.startedAtSecs * 1000).toLocaleString() }}</time><span>{{ turn.endReason ?? 'In progress' }}</span></div>
        <article class="message user"><h3>You <small>{{ turn.promptSource }}</small></h3><p>{{ turn.prompt ?? 'Prompt was not observed.' }}</p></article>
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
    <SessionComposer :key="session.id" :session-id="session.id" @raw="emit('raw')" />
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
