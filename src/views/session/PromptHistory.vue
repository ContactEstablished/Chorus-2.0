<script setup lang="ts">
/**
 * The session's prompts, newest first, each with a Copy button (PLAN P4). Opening one shows how the
 * turn ended, the agent's final message and its tool calls. Text is as recorded, already scrubbed.
 */
import { ref, watch } from 'vue'
import Icon from '../../components/Icon.vue'
import { commands, unwrap, type SessionHistory, type SessionSummary, type TurnEntry } from '../../ipc'

const props = defineProps<{ session: SessionSummary }>()
const history = ref<SessionHistory | null>(null)
const error = ref<string | null>(null)
const open = ref<number | null>(null)
const copied = ref<string | null>(null)
const SHOWN = 50

async function load(): Promise<void> {
  try {
    history.value = await unwrap(commands.sessionHistory(props.session.id, SHOWN))
    error.value = null
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
}

// Reload when a turn starts or ends (the activity changes) or the run changes.
watch(() => [props.session.id, props.session.activity, props.session.live, props.session.latestAtSecs], load, { immediate: true })

async function copy(key: string, text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text)
    copied.value = key
    setTimeout(() => {
      if (copied.value === key) copied.value = null
    }, 1500)
  } catch (e) {
    error.value = `Couldn't copy: ${e instanceof Error ? e.message : String(e)}`
  }
}

function time(secs: number): string {
  const d = new Date(secs * 1000)
  const today = new Date().toDateString() === d.toDateString()
  return today ? d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }) : d.toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
}

const END: Record<string, string> = { done: 'done', interrupted: 'interrupted', failed: 'failed', lost: 'lost' }
const endOf = (t: TurnEntry) => (t.endReason ? (END[t.endReason] ?? t.endReason) : 'in progress')
</script>

<template>
  <section class="history">
    <h3>Prompts <span v-if="history" class="n">{{ history.total }}</span></h3>
    <p v-if="error" class="error">{{ error }}</p>
    <p v-if="history && !history.turns.length" class="hint">
      {{ session.hasEventChannel ? 'No prompts yet. Each one you send is recorded here.' : 'Chorus records no turns for this agent yet.' }}
    </p>
    <ol v-if="history" class="turns">
      <li v-for="t in history.turns" :key="t.id" class="turn" :class="{ open: open === t.id }">
        <div class="line">
          <button class="prompt" :title="open === t.id ? 'Hide the turn' : 'Show the turn'" @click="open = open === t.id ? null : t.id">
            {{ t.prompt ?? '(prompt not seen)' }}
          </button>
          <button
            v-if="t.prompt"
            class="copy"
            :title="copied === `p${t.id}` ? 'Copied' : 'Copy the prompt'"
            :aria-label="`Copy prompt from ${time(t.startedAtSecs)}`"
            @click="copy(`p${t.id}`, t.prompt)"
          >
            <Icon :name="copied === `p${t.id}` ? 'check' : 'copy'" :size="14" />
          </button>
        </div>
        <p class="meta">
          {{ time(t.startedAtSecs) }} · <span :class="['end', t.endReason ?? 'open']">{{ endOf(t) }}</span>
          <template v-if="t.tools.length"> · {{ t.tools.length }} tool call{{ t.tools.length === 1 ? '' : 's' }}</template>
        </p>
        <div v-if="open === t.id" class="detail">
          <ul v-if="t.tools.length || t.asks.length">
            <li v-for="(a, i) in t.asks" :key="'a' + i">{{ a.kind === 'approval' ? 'Asked to' : 'Asked' }}: {{ a.subject }} → {{ a.answer ?? 'waiting' }}</li>
            <li v-for="(tool, i) in t.tools" :key="'t' + i" :class="{ failed: tool.failed }">
              {{ tool.tool }}<template v-if="tool.target"> · <code>{{ tool.target }}</code></template>
              <template v-if="!tool.finished"> (running)</template><template v-if="tool.failed"> (failed)</template>
            </li>
          </ul>
          <div v-if="t.finalMessage" class="final">
            <p>{{ t.finalMessage }}</p>
            <button class="quiet small" @click="copy(`m${t.id}`, t.finalMessage)">
              <Icon :name="copied === `m${t.id}` ? 'check' : 'copy'" :size="13" /> {{ copied === `m${t.id}` ? 'Copied' : 'Copy reply' }}
            </button>
          </div>
        </div>
      </li>
    </ol>
    <p v-if="history?.transcriptPath" class="transcript" :title="history.transcriptPath">Transcript: <code>{{ history.transcriptPath }}</code></p>
  </section>
</template>

<style scoped>
h3 {
  margin: 0 0 10px;
  font-family: var(--font-ui);
  font-size: 13px;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-2);
}
.n {
  color: var(--text-muted);
  font-weight: 500;
}
.hint,
.error {
  margin: 0;
  font-size: 13px;
  color: var(--text-muted);
}
.error {
  color: var(--status-failed);
}
.turns {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.turn {
  padding: 8px 10px;
  border-radius: 9px;
  background: var(--surface);
  border: 1px solid var(--border);
}
.line {
  display: flex;
  gap: 6px;
  align-items: flex-start;
}
.prompt {
  flex: 1;
  min-width: 0;
  padding: 0;
  border: 0;
  background: transparent;
  text-align: left;
  font-size: 13px;
  line-height: 1.45;
  color: var(--text);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  overflow-wrap: anywhere;
  white-space: pre-wrap;
}
.turn.open .prompt {
  display: block;
}
.copy {
  padding: 3px;
  border: 0;
  background: transparent;
  color: var(--text-muted);
  display: grid;
}
.copy:hover:not(:disabled) {
  color: var(--text);
}
.meta {
  margin: 4px 0 0;
  font-size: 11.5px;
  color: var(--text-muted);
}
.end.done {
  color: var(--status-ready);
}
.end.failed,
.end.lost {
  color: var(--status-failed);
}
.end.interrupted {
  color: var(--status-needs);
}
.end.open {
  color: var(--status-working);
}
.detail {
  margin-top: 8px;
  padding-top: 8px;
  border-top: 1px solid var(--border);
  font-size: 12.5px;
  color: var(--text-2);
}
.detail ul {
  margin: 0 0 8px;
  padding-left: 16px;
}
.detail .failed {
  color: var(--status-failed);
}
.detail code {
  font-size: 11.5px;
  overflow-wrap: anywhere;
}
.final p {
  margin: 0 0 6px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  max-height: 240px;
  overflow: auto;
}
.small {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 2px 6px;
  font-size: 12px;
}
.transcript {
  margin: 10px 0 0;
  font-size: 11.5px;
  color: var(--text-muted);
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
</style>
