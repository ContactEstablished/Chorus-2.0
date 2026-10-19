<script setup lang="ts">
/** Starts a session: which agent, in which folder, optionally with a stored key as a variable. */
import { computed, ref } from 'vue'
import type { AgentKind, CreateSession, CredentialSummary } from '../ipc'

const props = defineProps<{ keys: CredentialSummary[]; busy: boolean }>()
const emit = defineEmits<{ create: [request: CreateSession]; cancel: [] }>()

const AGENTS: { kind: AgentKind; label: string }[] = [
  { kind: 'shell', label: 'Shell (cmd)' },
  { kind: 'claude', label: 'Claude Code' },
  { kind: 'codex', label: 'Codex' },
  { kind: 'opencode', label: 'opencode' },
  { kind: 'grok', label: 'Grok' },
]

const FOLDER_KEY = 'chorus.newSession.folder'
function remembered(): string {
  try {
    return localStorage.getItem(FOLDER_KEY) ?? ''
  } catch {
    return ''
  }
}

const agent = ref<AgentKind>('shell')
const cwd = ref(remembered())
const name = ref('')
const keyId = ref<number | null>(null)
const envVar = ref('')

const canSubmit = computed(() => cwd.value.trim() !== '' && (keyId.value === null || envVar.value.trim() !== ''))

function submit(): void {
  if (!canSubmit.value || props.busy) return
  try {
    localStorage.setItem(FOLDER_KEY, cwd.value.trim())
  } catch {
    // A remembered folder is a convenience; without storage the field just starts empty.
  }
  emit('create', {
    agent: agent.value,
    cwd: cwd.value,
    name: name.value.trim() || null,
    credential: keyId.value === null ? null : { id: keyId.value, envVar: envVar.value.trim() },
    cols: 120,
    rows: 30,
  })
}
</script>

<template>
  <form class="panel" @submit.prevent="submit">
    <h2>New session</h2>
    <label>
      <span>Agent</span>
      <select v-model="agent">
        <option v-for="a in AGENTS" :key="a.kind" :value="a.kind">{{ a.label }}</option>
      </select>
    </label>
    <label>
      <span>Folder</span>
      <input v-model="cwd" placeholder="C:\Projects\app" spellcheck="false" autocomplete="off" />
    </label>
    <label>
      <span>Name <em>(optional)</em></span>
      <input v-model="name" maxlength="80" autocomplete="off" />
    </label>
    <label>
      <span>Key <em>(optional)</em></span>
      <select v-model="keyId">
        <option :value="null">None</option>
        <option v-for="k in keys" :key="k.id" :value="k.id">{{ k.label }} · {{ k.fingerprint }}</option>
      </select>
    </label>
    <label v-if="keyId !== null">
      <span>As variable</span>
      <input v-model="envVar" placeholder="OPENROUTER_API_KEY" spellcheck="false" autocomplete="off" />
    </label>
    <p class="hint">The key reaches the process only as this environment variable, and is redacted from its output.</p>
    <div class="actions">
      <button type="submit" class="primary" :disabled="!canSubmit || busy">{{ busy ? 'Starting…' : 'Start' }}</button>
      <button type="button" @click="emit('cancel')">Cancel</button>
    </div>
  </form>
</template>

<style scoped>
.panel {
  max-width: 520px;
  display: grid;
  gap: 14px;
}
h2 {
  font-family: var(--font-heading);
  font-size: 20px;
  margin: 0 0 4px;
}
label {
  display: grid;
  gap: 6px;
}
label span {
  color: var(--text-2);
  font-size: 13px;
}
em {
  color: var(--text-muted);
  font-style: normal;
}
.hint {
  color: var(--text-muted);
  font-size: 12.5px;
  margin: 0;
}
.actions {
  display: flex;
  gap: 10px;
}
</style>
