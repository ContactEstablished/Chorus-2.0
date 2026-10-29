<script setup lang="ts">
/**
 * The Friendly conversation: a scrolling thread of exchanges (what you sent, what the agent did,
 * what it replied) above a docked composer that stays within reach however far back you read.
 * Everything shown comes from recorded history; nothing here infers session state.
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { commands, unwrap, type SessionHistory, type SessionSummary, type PendingReply, type NarratorView, type TurnEntry } from '../../ipc'
import NarratorPanel from './NarratorPanel.vue'
import ReplyCard from './ReplyCard.vue'
import MessageContent from '../../components/MessageContent.vue'
import FriendlyComposer from './FriendlyComposer.vue'
import GrillCard from './GrillCard.vue'
import { parseGrill } from './grill'
import Icon from '../../components/Icon.vue'
import { AGENT_LOGOS } from '../../components/agentLogos'
import { AGENT_LABELS } from '../../raw/labels'
import { duration, endLabel, eventCount, hasWork, isLongPrompt, latestPlan, sourceBadge, steps, systemNotice, timeLabel, workSummary } from './conversation'

const props = defineProps<{ session: SessionSummary }>()
const emit = defineEmits<{ raw: [] }>()
const PAGE = 100
const MAX = 500 // The back end's own cap on one history read.
const limit = ref(PAGE)
const history = ref<SessionHistory | null>(null)
const pending = ref<PendingReply[]>([])
const error = ref('')
const narrator = ref<NarratorView | null>(null)
const narratorError = ref('')
const viewport = ref<HTMLElement | null>(null)
const content = ref<HTMLElement | null>(null)
const agentLabel = computed(() => AGENT_LABELS[props.session.agent])
/** Long prompts and system notices the reader has opened, by turn id. */
const expanded = ref(new Set<number>())
function toggle(id: number): void {
  if (expanded.value.has(id)) expanded.value.delete(id)
  else expanded.value.add(id)
}
/** A grill round in the newest reply: its questions, each with the agent's recommended answer. */
const latestTurn = computed(() => history.value?.turns.at(-1))
const grill = computed(() => (latestTurn.value?.endReason ? parseGrill(latestTurn.value.finalMessage) : []))
const notice = (turn: TurnEntry) => systemNotice(turn.prompt)

// The view sticks to the newest content until the reader scrolls up; then a button offers the way
// back and counts what arrived meanwhile.
const following = ref(true)
const seen = ref(0)
const unseen = computed(() => Math.max(0, eventCount(history.value?.turns ?? []) - seen.value))
let resizeObserver: ResizeObserver | undefined
function markSeen(): void { seen.value = eventCount(history.value?.turns ?? []) }
function trackScroll(): void {
  const el = viewport.value
  if (!el) return
  following.value = el.scrollHeight - el.scrollTop - el.clientHeight < 80
  if (following.value) markSeen()
}
function followLatest(): void {
  const el = viewport.value
  if (following.value && el) { el.scrollTop = el.scrollHeight; markSeen() }
}
function jumpToLatest(): void { following.value = true; followLatest() }
const flash = ref<number | null>(null)
/** Scrolls one turn into view and highlights it briefly (the details panel's "Show in conversation"). */
async function scrollToTurn(id: number): Promise<void> {
  await nextTick()
  const el = viewport.value?.querySelector<HTMLElement>(`#turn-${id}`)
  if (!el) return
  following.value = false
  el.scrollIntoView({ block: 'start', behavior: 'smooth' })
  flash.value = id
  setTimeout(() => { if (flash.value === id) flash.value = null }, 1800)
}
defineExpose({ scrollToTurn })
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
    const [h, p] = await Promise.all([unwrap(commands.sessionHistory(id, limit.value)), unwrap(commands.sessionPendingReplies(id))])
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
/** Reads another page of older turns and keeps the reader's place on screen. */
async function loadEarlier(): Promise<void> {
  const el = viewport.value
  const fromBottom = el ? el.scrollHeight - el.scrollTop : 0
  limit.value = Math.min(MAX, limit.value + PAGE)
  following.value = false
  await load()
  await nextTick()
  if (el) el.scrollTop = el.scrollHeight - fromBottom
}
watch(() => props.session.id, () => {
  following.value = true; seen.value = 0; limit.value = PAGE; expanded.value.clear()
  history.value = null; pending.value = []; narrator.value = null; narratorError.value = ''
  void load()
}, { immediate: true })
onBeforeUnmount(() => { disposed = true; generation++; clearTimeout(timer); resizeObserver?.disconnect() })
</script>

<template>
  <div class="friendly">
    <div class="thread">
      <div ref="viewport" class="conversation scroll" @scroll="trackScroll">
        <div ref="content" class="content">
          <p v-if="error" class="error" role="alert">History could not refresh: {{ error }}. <button class="quiet" @click="emit('raw')">Open Raw</button></p>
          <p v-if="history && !history.turns.length" class="empty">{{ session.hasEventChannel ? 'Nothing recorded yet. Your first message starts the conversation.' : 'This agent has no structured history channel. Use Raw.' }}</p>
          <div v-if="history && history.total > history.turns.length" class="earlier">
            <button class="quiet" :disabled="limit >= MAX" :title="limit >= MAX ? 'Chorus shows at most 500 turns here. Export the transcript for the rest.' : ''" @click="loadEarlier">
              Show earlier turns <span class="n">{{ history.total - history.turns.length }} more</span>
            </button>
          </div>
          <ol v-if="history" class="turns">
            <li v-for="turn in history.turns" :id="`turn-${turn.id}`" :key="turn.id" class="turn" :class="{ live: !turn.endReason, flash: flash === turn.id }">
              <div class="divider" role="separator">
                <span class="when">{{ timeLabel(turn.startedAtSecs) }}</span>
                <span v-if="duration(turn.startedAtSecs, turn.endedAtSecs)" class="dur">· {{ duration(turn.startedAtSecs, turn.endedAtSecs) }}</span>
                <span class="end" :class="endLabel(turn).cls"><span v-if="!turn.endReason" class="dot pulse st-working" />{{ endLabel(turn).label }}</span>
              </div>

              <!-- What you sent. A harness-made prompt is a quiet system line, not your words. -->
              <div v-if="notice(turn)" class="system">
                <button class="quiet sys-line" :aria-expanded="expanded.has(turn.id)" @click="toggle(turn.id)">
                  <Icon name="info" :size="14" /><span class="sys-title">{{ notice(turn)!.title }}</span><Icon :name="expanded.has(turn.id) ? 'chevronDown' : 'chevronRight'" :size="13" />
                </button>
                <pre v-if="expanded.has(turn.id)">{{ notice(turn)!.detail }}</pre>
              </div>
              <div v-else class="sent-row">
                <article class="sent" :class="{ folded: isLongPrompt(turn.prompt) && !expanded.has(turn.id) }">
                  <MessageContent :text="turn.prompt ?? 'Prompt was not observed.'">You<small v-if="sourceBadge(turn.promptSource)">{{ sourceBadge(turn.promptSource) }}</small></MessageContent>
                  <button v-if="isLongPrompt(turn.prompt)" class="quiet fold" @click="toggle(turn.id)">{{ expanded.has(turn.id) ? 'Show less' : 'Show the full prompt' }}</button>
                </article>
              </div>

              <!-- What the agent did: open while it works, folded once it has replied. -->
              <details v-if="hasWork(turn)" class="work" :open="!turn.endReason">
                <summary>
                  <span class="dot" :class="turn.endReason ? 'st-done' : 'st-working pulse'" />
                  <span class="work-title">{{ turn.endReason ? 'What it did' : 'Working' }}</span>
                  <span class="work-sum">{{ workSummary(turn) }}</span>
                  <Icon class="caret" name="chevronDown" :size="14" />
                </summary>
                <div class="work-body">
                  <ol v-if="steps(turn).length" class="steps">
                    <li v-for="step in steps(turn)" :key="step.id" :class="step.kind">
                      <span v-if="step.kind === 'proposed-plan'" class="step-tag">Proposed plan</span>
                      <MessageContent :text="step.text" plain />
                    </li>
                  </ol>
                  <section v-if="latestPlan(turn)" class="plan" aria-label="Agent plan">
                    <h3>Plan</h3>
                    <ol><li v-for="step in latestPlan(turn)" :key="step.id" :class="step.status">
                      <span class="step-status">{{ step.status === 'completed' ? '✓ Done' : step.status === 'in-progress' ? '● In progress' : '○ Pending' }}</span>
                      <span>{{ step.text }}</span>
                    </li></ol>
                  </section>
                  <article v-for="ask in turn.asks" :key="ask.id" class="ask">
                    <h3>{{ ask.kind === 'approval' ? 'Approval' : 'Question' }}</h3>
                    <p>{{ ask.subject }}</p>
                    <p v-if="ask.replyAction?.kind === 'approval'">You requested: {{ ask.replyAction.decision === 'allow' ? 'allow once' : 'deny' }}<template v-if="ask.replyAction.message"> — {{ ask.replyAction.message }}</template></p>
                    <p v-else-if="ask.replyAction?.kind === 'answer'">You sent: {{ ask.replyAction.answers.join(' · ') }}</p>
                    <p v-else-if="ask.replyAction?.kind === 'release'">Returned to Raw without a decision.</p>
                    <p>{{ ask.answer ?? (ask.replyAttempted ? 'Reply attempted; agent outcome not yet observed.' : 'No answer observed. Use the active card below, or Raw.') }}</p>
                    <details v-if="ask.details"><summary>Full request</summary><pre>{{ ask.details.kind === 'approval' ? ask.details.input : JSON.stringify(ask.details.questions, null, 2) }}</pre></details>
                  </article>
                  <details v-if="turn.tools.length" class="tools">
                    <summary>{{ turn.tools.length }} tool {{ turn.tools.length === 1 ? 'call' : 'calls' }}</summary>
                    <ul><li v-for="(tool, i) in turn.tools" :key="i" :class="{ failed: tool.failed }"><strong>{{ tool.tool }}</strong> · {{ tool.failed ? 'Failed' : tool.finished ? 'Finished' : 'Started' }}<code v-if="tool.target">{{ tool.target }}</code></li></ul>
                  </details>
                </div>
              </details>

              <!-- What it replied. -->
              <div v-if="turn.finalMessage" class="reply-row">
                <span class="avatar agent-chip" :class="`agent-${session.agent}`" :title="agentLabel">
                  <img :src="AGENT_LOGOS[session.agent]" :alt="agentLabel" />
                </span>
                <article class="reply">
                  <MessageContent :text="turn.finalMessage">{{ agentLabel }}<small v-if="turn.endedAtSecs">{{ timeLabel(turn.endedAtSecs) }}</small></MessageContent>
                </article>
              </div>
              <p v-else-if="turn.endReason && turn.endReason !== 'done'" class="noreply">The turn {{ endLabel(turn).label.toLowerCase() }} without a reply.</p>
              <p v-else-if="turn.endReason === 'done'" class="noreply">The turn ended without a recorded reply.</p>
            </li>
          </ol>
        </div>
      </div>
      <button v-if="!following && history?.turns.length" class="jump" :title="unseen ? 'New content arrived below' : 'Back to the latest message'" @click="jumpToLatest">
        <Icon name="chevronDown" :size="14" />{{ unseen ? `${unseen} new` : 'Latest' }}
      </button>
    </div>

    <div class="dock">
      <div class="dock-inner">
        <div v-if="pending.length" class="pending">
          <ReplyCard v-for="p in pending" :key="`${p.runId}-${p.askId}`" :session-id="session.id" :pending="p" @refresh="load" @raw="emit('raw')" />
        </div>
        <GrillCard v-if="grill.length" :key="`${session.id}-${latestTurn?.id}`" :session-id="session.id" :questions="grill" />
        <NarratorPanel :session="session" :narrator="narrator" :error="narratorError" @refresh="load" />
        <FriendlyComposer :session-id="session.id" @raw="emit('raw')" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.friendly { height: 100%; min-height: 0; min-width: 0; display: flex; flex-direction: column; }
.thread { position: relative; flex: 1; min-height: 0; display: flex; flex-direction: column; }
.conversation { flex: 1; min-height: 0; padding: 18px clamp(18px, 3vw, 40px) 16px; }
.content { max-width: 920px; margin: 0 auto; }
.empty, .error { font-size: 13px; color: var(--text-muted); line-height: 1.6; }
.error { color: var(--status-failed); }
.earlier { display: flex; justify-content: center; margin: 0 0 10px; }
.earlier button { font-size: 12.5px; }
.earlier .n { margin-left: 6px; color: var(--text-muted); }

/* Each exchange: a dated divider, what you sent, what the agent did, what it replied. Every second
   reply card sits on a slightly lighter surface so the eye catches where one exchange ends. */
.turns { list-style: none; margin: 0; padding: 0; }
.turn:nth-child(even) .reply { background: var(--surface-raised-alt); }
.turn { display: grid; gap: 10px; padding: 6px 8px 24px; margin: 0 -8px; border-radius: 12px; transition: background 0.6s; }
.turn.flash { background: var(--accent-soft); transition: none; }
.divider { display: flex; align-items: center; justify-content: center; gap: 8px; margin: 6px 0 2px; font-size: 11.5px; color: var(--text-muted); }
.divider::before, .divider::after { content: ''; flex: 1; height: 1px; background: var(--border); }
.end { display: inline-flex; align-items: center; gap: 6px; padding: 1px 9px; border-radius: var(--radius-pill); background: var(--surface-chip); font-weight: 600; }
.end .dot { width: 6px; height: 6px; }
.end.done { color: var(--status-ready); }
.end.failed { color: var(--status-failed); }
.end.interrupted { color: var(--status-needs); }
.end.open { color: var(--status-working); }

/* Sent: on the right, in the accent, as a message you wrote. */
.sent-row { display: flex; justify-content: flex-end; }
.sent { position: relative; max-width: min(78%, 680px); min-width: 0; padding: 14px 18px 12px; background: color-mix(in srgb, var(--accent) 9%, var(--surface-raised)); border: 1px solid var(--accent-line); border-radius: 16px 16px 4px 16px; }
.sent :deep(.message-label) { color: var(--text-2); }
.sent :deep(.message-toolbar) { margin-bottom: 8px; }
.sent.folded :deep(.message-content), .sent.folded :deep(.message-source) { max-height: 200px; overflow: hidden; mask-image: linear-gradient(to bottom, #000 55%, transparent); }
.fold { font-size: 12px; padding: 2px 8px; margin: 4px 0 0 -8px; color: var(--accent); }
.sent small, .reply small { margin-left: 8px; color: var(--text-muted); font-weight: 400; letter-spacing: 0; }

/* A harness-made prompt: one quiet centred line, its raw text on request. */
.system { display: grid; justify-items: center; gap: 6px; }
.sys-line { display: inline-flex; align-items: center; gap: 8px; max-width: 100%; padding: 4px 12px; border-radius: var(--radius-pill); font-size: 12px; color: var(--text-muted); }
.sys-title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.system pre { width: 100%; margin: 0; padding: 12px 14px; font: 12px/1.6 var(--font-mono); color: var(--text-2); background: var(--surface); border: 1px solid var(--border); border-radius: 10px; white-space: pre-wrap; overflow-wrap: anywhere; max-height: 320px; overflow: auto; }

/* The agent's work: a folded rail between your message and its reply. */
.work { margin-left: 44px; padding-left: 16px; border-left: 2px solid var(--border-strong); }
.turn.live .work { border-left-color: var(--accent-line); }
.work > summary { display: flex; align-items: center; gap: 10px; padding: 4px 0; list-style: none; cursor: pointer; font-size: 12.5px; color: var(--text-2); }
.work > summary::-webkit-details-marker { display: none; }
.work-title { font-weight: 600; }
.work-sum { color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.caret { margin-left: auto; color: var(--text-muted); transition: transform 0.15s; }
.work[open] > summary .caret { transform: rotate(180deg); }
.work-body { display: grid; gap: 12px; padding: 8px 0 4px; }
.steps { list-style: none; margin: 0; padding: 0; display: grid; gap: 10px; }
.steps li { position: relative; padding-left: 16px; min-width: 0; }
.steps li::before { content: ''; position: absolute; left: 1px; top: 0.7em; width: 5px; height: 5px; border-radius: 50%; background: var(--text-muted); }
.turn.live .steps li:last-child::before { background: var(--accent); }
.steps :deep(.message-content) { font-size: 13px; line-height: 1.65; color: var(--text-2); }
.step-tag { display: block; margin-bottom: 2px; font-size: 11px; font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; color: var(--accent); }
.plan { padding: 14px 18px; background: var(--surface); border: 1px solid var(--border); border-radius: 12px; }
.plan h3 { margin: 0 0 10px; font-size: 13px; }
.plan ol { list-style: none; padding: 0; margin: 0; }
.plan li { display: flex; align-items: baseline; gap: 12px; margin: 6px 0; font-size: 13px; line-height: 1.5; }
.step-status { min-width: 95px; flex-shrink: 0; color: var(--text-muted); font-size: 12px; }
.in-progress .step-status { color: var(--accent); }
.completed .step-status { color: var(--status-ready); }
.ask { padding: 14px 18px; background: var(--surface); border: 1px solid var(--border); border-left: 3px solid var(--status-input); border-radius: 12px; }
.ask h3 { margin: 0 0 8px; font-size: 12px; letter-spacing: 0.04em; color: var(--status-input); }
.ask p { white-space: pre-wrap; overflow-wrap: anywhere; margin: 4px 0; font-size: 13px; line-height: 1.55; color: var(--text-2); }
.tools { font-size: 12px; color: var(--text-2); }
.tools summary { cursor: pointer; }
.tools li { margin: 6px 0; }
.tools .failed { color: var(--status-failed); }
code { display: block; overflow-wrap: anywhere; margin: 2px 0; font-size: 11.5px; }
pre { white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; max-height: 320px; overflow: auto; }

/* Reply: on the left, beside the agent's chip, the widest and strongest card. */
.reply-row { display: grid; grid-template-columns: 32px minmax(0, 1fr); gap: 12px; align-items: start; }
.avatar { width: 32px; height: 32px; min-width: 32px; margin-top: 4px; border-radius: 8px; font-size: 11px; }
.avatar { padding: 0; }
.avatar img { width: 24px; height: 24px; object-fit: contain; }
.reply { min-width: 0; padding: 18px 22px; background: var(--surface-raised); border: 1px solid var(--border); border-top: 2px solid var(--accent-line); border-radius: 4px 16px 16px 16px; }
.reply :deep(.message-label) { color: var(--accent); }
.noreply { margin: 0 0 0 44px; font-size: 12.5px; font-style: italic; color: var(--text-muted); }

/* Copy and Source stay out of the way until a card is pointed at or focused. */
.sent :deep(.message-actions), .reply :deep(.message-actions) { opacity: 0; transition: opacity 0.15s; }
.sent:hover :deep(.message-actions), .sent:focus-within :deep(.message-actions),
.reply:hover :deep(.message-actions), .reply:focus-within :deep(.message-actions) { opacity: 1; }

.jump { position: absolute; left: 50%; bottom: 12px; transform: translateX(-50%); display: inline-flex; align-items: center; gap: 6px; padding: 6px 14px; border-radius: var(--radius-pill); background: var(--accent); border-color: var(--accent); color: var(--on-accent); font-size: 12.5px; font-weight: 600; box-shadow: 0 6px 18px rgb(0 0 0 / 0.35); }
.jump:hover { background: var(--accent-hover); }

/* The dock: requests, the glance bar and the composer, always in reach. */
.dock { flex: none; max-height: 62%; overflow: auto; padding: 10px clamp(18px, 3vw, 40px) 14px; border-top: 1px solid var(--border); background: var(--bg); scrollbar-width: thin; }
.dock-inner { max-width: 920px; margin: 0 auto; display: grid; grid-template-columns: minmax(0, 1fr); gap: 10px; }
.pending { display: grid; gap: 10px; }

@media (max-width: 720px) {
  .conversation, .dock { padding-left: 14px; padding-right: 14px; }
  .sent { max-width: 94%; }
  .work, .noreply { margin-left: 0; }
  .reply-row { grid-template-columns: minmax(0, 1fr); }
  .avatar { display: none; }
  .reply { border-radius: 16px; }
}
</style>
