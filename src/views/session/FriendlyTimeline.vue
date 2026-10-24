<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { commands, unwrap, type SessionHistory, type SessionSummary, type PendingReply, type NarratorView } from '../../ipc'
import NarratorPanel from './NarratorPanel.vue'
import ReplyCard from './ReplyCard.vue'
import MessageContent from '../../components/MessageContent.vue'
import FriendlyComposer from './FriendlyComposer.vue'

const props = defineProps<{ session: SessionSummary }>()
const emit = defineEmits<{ raw: [] }>()
const history = ref<SessionHistory | null>(null)
const pending = ref<PendingReply[]>([])
const error = ref('')
const narrator = ref<NarratorView | null>(null)
const narratorError = ref('')
const viewport = ref<HTMLElement | null>(null)
const content = ref<HTMLElement | null>(null)
let following = true
let resizeObserver: ResizeObserver | undefined
function trackScroll(): void {
  const el = viewport.value
  if (el) following = el.scrollHeight - el.scrollTop - el.clientHeight < 80
}
function followLatest(): void {
  const el = viewport.value
  if (following && el) el.scrollTop = el.scrollHeight
}
onMounted(() => {
  // Also follow content that grows after rendering, such as Markdown images.
  resizeObserver = new ResizeObserver(followLatest)
  if (content.value) resizeObserver.observe(content.value)
  followLatest()
})
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
    // A missing narrator backend must not stop timeline/reply refresh during a dev update.
    try {
      const n = await unwrap(commands.sessionNarrator(id))
      if (!disposed && current === generation) { narrator.value = n; narratorError.value = '' }
    } catch (e) { if (!disposed && current === generation) narratorError.value = /command.*not found|unknown command/i.test(String(e)) ? 'Restart Chorus 2 to load the updated narrator backend.' : String(e) }
    await nextTick()
    if (!disposed && current === generation) followLatest()
  } catch (e) {
    if (current === generation) { error.value = String(e); pending.value = [] }
  } finally {
    if (!disposed && current === generation) timer = setTimeout(() => void load(), 1500)
  }
}
watch(() => props.session.id, () => { following = true; history.value = null; pending.value = []; narrator.value = null; narratorError.value = ''; void load() }, { immediate: true })
onBeforeUnmount(() => { disposed = true; generation++; clearTimeout(timer); resizeObserver?.disconnect() })
</script>

<template>
  <div ref="viewport" class="friendly scroll" @scroll="trackScroll">
    <div ref="content" class="content">
    <div class="intro">
      <h2>Session timeline</h2>
      <p>Recorded messages, tools and requests from this running session. Raw shows the live terminal.</p>
    </div>
    <p v-if="error" class="error" role="alert">History could not refresh: {{ error }}. <button @click="emit('raw')">Open Raw</button></p>
    <p v-if="history && !history.turns.length" class="empty">{{ session.hasEventChannel ? 'Waiting for the first recorded prompt.' : 'This agent has no structured history channel. Use Raw.' }}</p>
    <ol v-if="history" class="timeline">
      <li v-for="turn in history.turns" :key="turn.id" class="turn">
        <div class="meta"><time>{{ new Date(turn.startedAtSecs * 1000).toLocaleString() }}</time><span>{{ turn.endReason ?? 'In progress' }}</span></div>
        <article class="message user"><MessageContent :text="turn.prompt ?? 'Prompt was not observed.'">You <small>{{ turn.promptSource }}</small></MessageContent></article>
        <template v-for="update in (turn.progress ?? [])" :key="update.id">
          <article v-if="update.content.kind === 'message' && update.content.text.trim() !== turn.finalMessage?.trim()" class="message agent progress">
            <MessageContent :text="update.content.text">Agent · Progress</MessageContent>
          </article>
          <article v-else-if="update.content.kind === 'proposed-plan'" class="message agent">
            <MessageContent :text="update.content.text">Proposed plan</MessageContent>
          </article>
        </template>
        <template v-for="update in (turn.progress ?? []).filter(p => p.content.kind === 'plan').slice(-1)" :key="`plan-${update.id}`">
          <section v-if="update.content.kind === 'plan' && update.content.steps.length" class="plan" aria-label="Agent plan">
            <h3>Plan</h3>
            <ol><li v-for="step in update.content.steps" :key="step.id" :class="step.status">
              <span class="step-status">{{ step.status === 'completed' ? '✓ Done' : step.status === 'in-progress' ? '● In progress' : '○ Pending' }}</span>
              <span>{{ step.text }}</span>
            </li></ol>
          </section>
        </template>
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
          <p>{{ ask.answer ?? (ask.replyAttempted ? 'Reply attempted; agent outcome not yet observed.' : 'No answer observed. Use the active card below, or Raw.') }}</p>
          <details v-if="ask.details"><summary>Full request</summary><pre>{{ ask.details.kind === 'approval' ? ask.details.input : JSON.stringify(ask.details.questions, null, 2) }}</pre></details>
        </article>
        <article v-if="turn.finalMessage" class="message agent"><MessageContent :text="turn.finalMessage">Agent</MessageContent></article>
      </li>
    </ol>
    <p v-if="history && history.total > history.turns.length" class="limitation">Showing the latest {{ history.turns.length }} of {{ history.total }} turns.</p>
    <div v-if="pending.length" class="pending">
      <ReplyCard v-for="p in pending" :key="`${p.runId}-${p.askId}`" :session-id="session.id" :pending="p" @refresh="load" @raw="emit('raw')" />
    </div>
    <NarratorPanel :session="session" :narrator="narrator" :error="narratorError" @refresh="load" />
    <FriendlyComposer :session-id="session.id" @raw="emit('raw')" />
    </div>
  </div>
</template>

<style scoped>
.friendly { padding: 24px clamp(18px, 3vw, 40px); min-height: 0; min-width: 0; }
.content { max-width: 920px; margin-left: auto; margin-right: auto; display: flow-root; }
h2 { margin: 0; font-size: 19px; }
.intro p, .empty, .limitation { font-size: 13px; color: var(--text-muted); line-height: 1.6; }
.limitation { padding: 12px; border: 1px dashed var(--border-strong); border-radius: 10px; }
.pending { display: grid; gap: 14px; margin: 18px 0; }
.timeline { list-style: none; margin: 20px auto; padding: 0; }
.turn { display: grid; gap: 12px; margin-bottom: 28px; }
.meta { display: flex; justify-content: space-between; gap: 12px; color: var(--text-muted); font-size: 12px; }
.message, .ask { padding: 20px 24px; min-width: 0; background: var(--surface-raised); border: 1px solid var(--border); border-radius: 14px; }
.user { background: var(--surface); border-left: 3px solid var(--border-strong); }
.agent { border-top: 2px solid var(--accent-line); }
.plan { padding: 16px 20px; background: var(--surface); border: 1px solid var(--border); border-radius: 12px; }
.plan h3 { margin: 0 0 12px; font-size: 14px; }
.plan ol { list-style: none; padding: 0; margin: 0; }
.plan li { display: flex; align-items: baseline; gap: 12px; margin: 8px 0; font-size: 13px; line-height: 1.5; }
.step-status { min-width: 95px; flex-shrink: 0; color: var(--text-muted); font-size: 12px; }
.in-progress .step-status { color: var(--accent); }
.completed .step-status { color: var(--status-ready); }
.message > h3, .ask > h3 { margin: 0 0 14px; font-size: 12px; letter-spacing: 0.04em; color: var(--text-2); }
.agent :deep(.message-label) { color: var(--accent); }
.user small { color: var(--text-muted); font-weight: 400; margin-left: 8px; }
article p { white-space: pre-wrap; overflow-wrap: anywhere; margin: 6px 0; font-size: 14px; line-height: 1.6; }
.tools { border-left: 2px solid var(--border-strong); padding: 8px 14px; font-size: 12px; color: var(--text-2); }
summary { cursor: pointer; }
.tools li { margin: 8px 0; }
code { display: block; overflow-wrap: anywhere; margin: 4px 0; }
pre { white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; max-height: 320px; overflow: auto; }
.error { color: var(--status-failed); }
@media (max-width: 720px) {
  .friendly { padding: 16px; }
  .message, .ask { padding: 16px; }
}
</style>
