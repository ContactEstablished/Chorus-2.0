<script setup lang="ts">
/**
 * A session's recorded turns (D7): each prompt, how the turn ended, the agent's final message, its
 * tool calls and any question or approval. The Friendly view (P6) replaces this bare list.
 */
import { ref, watch } from 'vue'
import { commands, unwrap, type SessionHistory, type SessionSummary } from '../ipc'

const props = defineProps<{ session: SessionSummary }>()
const history = ref<SessionHistory | null>(null)
const error = ref<string | null>(null)
const SHOWN = 30

async function load(): Promise<void> {
  try {
    history.value = await unwrap(commands.sessionHistory(props.session.id, SHOWN))
    error.value = null
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
}

// Reload when the session or what its agent is doing changes (a turn started or ended).
watch(() => [props.session.id, props.session.activity, props.session.live], load, { immediate: true })

function time(secs: number): string {
  return new Date(secs * 1000).toLocaleTimeString()
}

function short(text: string | null, max = 400): string {
  if (!text) return ''
  return text.length > max ? `${text.slice(0, max)}…` : text
}

const END_LABELS: Record<string, string> = {
  done: 'done',
  interrupted: 'interrupted',
  failed: 'failed',
  lost: 'lost (the run ended)',
}
</script>

<template>
  <section class="history">
    <p v-if="error" class="error">{{ error }}</p>
    <p v-if="history" class="counts">
      {{ history.total }} turn{{ history.total === 1 ? '' : 's' }} recorded · {{ history.ended }} with an end ·
      {{ history.withPrompt }} with the prompt
    </p>
    <p v-if="history && !history.turns.length" class="hint">
      No turns yet. Turns are recorded from the agent's own events: Claude Code and Codex through hooks, opencode through
      its server.
    </p>
    <ol v-if="history" class="turns">
      <li v-for="t in history.turns" :key="t.id" class="turn">
        <p class="when">
          {{ time(t.startedAtSecs) }}
          <span :class="['end', t.endReason ?? 'open']">{{ t.endReason ? END_LABELS[t.endReason] ?? t.endReason : 'in progress' }}</span>
        </p>
        <p class="prompt">{{ short(t.prompt) || '(prompt not seen)' }}</p>
        <ul v-if="t.tools.length || t.asks.length" class="detail">
          <li v-for="(a, i) in t.asks" :key="'a' + i">
            {{ a.kind === 'approval' ? 'Asked to' : 'Asked' }}: {{ a.subject }} → {{ a.answer ?? 'waiting' }}
          </li>
          <li v-for="(tool, i) in t.tools" :key="'t' + i" :class="{ failed: tool.failed }">
            {{ tool.tool }}<template v-if="tool.target"> · <code>{{ tool.target }}</code></template>
            <template v-if="!tool.finished"> (running)</template><template v-if="tool.failed"> (failed)</template>
          </li>
        </ul>
        <p v-if="t.finalMessage" class="message">{{ short(t.finalMessage, 600) }}</p>
      </li>
    </ol>
  </section>
</template>

<style scoped>
.history {
  display: grid;
  gap: 8px;
  max-height: 45vh;
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 10px 12px;
}
.counts,
.hint {
  margin: 0;
  color: var(--text-muted);
  font-size: 12.5px;
}
.error {
  margin: 0;
  color: var(--status-failed);
}
.turns {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  gap: 10px;
}
.turn {
  border-top: 1px solid var(--border);
  padding-top: 8px;
  display: grid;
  gap: 4px;
}
.when {
  margin: 0;
  font-size: 12px;
  color: var(--text-muted);
  display: flex;
  gap: 8px;
}
.end.done {
  color: var(--status-ready);
}
.end.interrupted,
.end.lost,
.end.open {
  color: var(--status-needs);
}
.end.failed {
  color: var(--status-failed);
}
.prompt {
  margin: 0;
  font-weight: 600;
  white-space: pre-wrap;
}
.message {
  margin: 0;
  color: var(--text-2);
  white-space: pre-wrap;
  font-size: 13px;
}
.detail {
  margin: 0;
  padding-left: 18px;
  font-size: 12.5px;
  color: var(--text-2);
}
.detail .failed {
  color: var(--status-failed);
}
code {
  font-family: var(--font-mono);
  font-size: 12px;
}
</style>
