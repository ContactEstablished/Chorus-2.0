<script setup lang="ts">
/**
 * Starts a session: which agent, in which folder, with which of the options that agent declares,
 * and optionally a stored key as a variable. Options an agent lacks are left out with the reason,
 * and each choice shows the exact arguments it adds.
 */
import { computed, onMounted, ref, watch } from 'vue'
import type { AgentKind, CreateSession, CredentialSummary, Effort, Permission } from '../ipc'
import { useAgentsStore } from '../stores/agents'
import { EFFORT_LABELS, PERMISSION_LABELS, detectionText } from './labels'

const props = defineProps<{ keys: CredentialSummary[]; busy: boolean }>()
const emit = defineEmits<{ create: [request: CreateSession]; cancel: [] }>()

const agents = useAgentsStore()
onMounted(() => agents.load(true))

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
// null = the agent's declared default.
const effort = ref<Effort | null>(null)
const permission = ref<Permission | null>(null)
const model = ref('')

const info = computed(() => agents.byKind.get(agent.value) ?? null)
const kinds = computed<AgentKind[]>(() =>
  agents.list.length ? agents.list.map((a) => a.kind) : ['shell', 'claude', 'codex', 'opencode', 'grok'],
)

// A choice made for one agent may not exist for the next.
watch(agent, () => {
  effort.value = null
  permission.value = null
  model.value = ''
})

const effortArgs = computed(() => {
  const chosen = effort.value ?? info.value?.defaultEffort ?? null
  return info.value?.efforts.find((e) => e.effort === chosen)?.args ?? ''
})
const permissionArgs = computed(() => {
  const chosen = permission.value ?? info.value?.defaultPermission ?? null
  return info.value?.permissions.find((p) => p.permission === chosen)?.args ?? ''
})

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
    options: { effort: effort.value, permission: permission.value, model: model.value.trim() || null },
  })
}
</script>

<template>
  <form class="panel" @submit.prevent="submit">
    <h2>New session</h2>
    <label>
      <span>Agent</span>
      <select v-model="agent">
        <option v-for="k in kinds" :key="k" :value="k">{{ agents.byKind.get(k)?.displayName ?? k }}</option>
      </select>
    </label>
    <p v-if="info" class="hint" :class="{ warn: info.detection && (!info.detection.found || info.detection.fit !== 'verified') && info.kind !== 'shell' }">
      {{ detectionText(info) }}
    </p>
    <p v-else-if="agents.error" class="hint warn">{{ agents.error }}</p>
    <label>
      <span>Folder</span>
      <input v-model="cwd" placeholder="C:\Projects\app" spellcheck="false" autocomplete="off" />
    </label>
    <label>
      <span>Name <em>(optional)</em></span>
      <input v-model="name" maxlength="80" autocomplete="off" />
    </label>

    <template v-if="info && info.kind !== 'shell'">
      <label v-if="info.efforts.length">
        <span>Effort</span>
        <select v-model="effort">
          <option :value="null">Default{{ info.defaultEffort ? ` (${EFFORT_LABELS[info.defaultEffort]})` : ': the CLI decides' }}</option>
          <option v-for="e in info.efforts" :key="e.effort" :value="e.effort">{{ EFFORT_LABELS[e.effort] }}</option>
        </select>
        <code v-if="effortArgs" class="args">{{ effortArgs }}</code>
      </label>
      <p v-else class="hint">{{ info.displayName }} has no effort setting Chorus can pass at launch yet.</p>

      <label v-if="info.permissions.length">
        <span>Permissions</span>
        <select v-model="permission">
          <option :value="null">
            Default{{ info.defaultPermission ? ` (${PERMISSION_LABELS[info.defaultPermission]})` : ': the CLI decides' }}
          </option>
          <option v-for="p in info.permissions" :key="p.permission" :value="p.permission">{{ PERMISSION_LABELS[p.permission] }}</option>
        </select>
        <code v-if="permissionArgs" class="args">{{ permissionArgs }}</code>
      </label>
      <p v-else class="hint">{{ info.displayName }} has no permission mode Chorus can pass at launch yet.</p>

      <label v-if="info.takesModel">
        <span>Model <em>(optional: the CLI's own default otherwise)</em></span>
        <input v-model="model" maxlength="200" spellcheck="false" autocomplete="off" />
      </label>
    </template>

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
.hint.warn {
  color: var(--status-needs);
}
.args {
  color: var(--text-muted);
  font-size: 12px;
}
.actions {
  display: flex;
  gap: 10px;
}
</style>
