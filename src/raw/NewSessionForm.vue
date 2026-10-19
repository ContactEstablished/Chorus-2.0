<script setup lang="ts">
/**
 * Starts a session: which agent, in which folder, with which of the options that agent declares,
 * and optionally a stored key as a variable. Options an agent lacks are left out with the reason,
 * and each choice shows the exact arguments it adds (for opencode, the server configuration).
 */
import { computed, onMounted, ref, watch } from 'vue'
import type { AgentKind, CreateSession, CredentialSummary, Effort, Permission } from '../ipc'
import { useAgentsStore } from '../stores/agents'
import { useProvidersStore } from '../stores/providers'
import { EFFORT_LABELS, PERMISSION_LABELS, detectionText, money } from './labels'

const props = defineProps<{ keys: CredentialSummary[]; busy: boolean }>()
const emit = defineEmits<{ create: [request: CreateSession]; cancel: [] }>()

const agents = useAgentsStore()
const providers = useProvidersStore()
onMounted(() => {
  void agents.load(true)
  void providers.load()
})

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
// null = the agent's own login, or a key given below.
const providerId = ref<number | null>(null)

const info = computed(() => agents.byKind.get(agent.value) ?? null)
const kinds = computed<AgentKind[]>(() =>
  agents.list.length ? agents.list.map((a) => a.kind) : ['shell', 'claude', 'codex', 'opencode', 'grok'],
)

// A choice made for one agent may not exist for the next.
watch(agent, () => {
  effort.value = null
  permission.value = null
  model.value = ''
  providerId.value = null
})

// Every provider an agent could run on is listed; the ones this agent can't use say why.
const routes = computed(() =>
  providers.list
    .filter((p) => p.kind !== 'jev')
    .map((p) => ({ provider: p, reason: p.agents.find((a) => a.agent === agent.value)?.reason ?? null })),
)
const provider = computed(() => (providerId.value === null ? null : (providers.byId.get(providerId.value) ?? null)))
const keyNote = computed(() => {
  const p = provider.value
  if (!p) return ''
  if (p.kind === 'openrouter' && p.managementCredentialId !== null) {
    return `Each run gets its own OpenRouter key, capped at ${money(p.capUsd ?? p.defaultCapUsd)}, and revoked when it ends.`
  }
  return `Runs use the key ${p.credentialLabel ?? '(none set)'}, without a cap.`
})

const effortArgs = computed(() => {
  const chosen = effort.value ?? info.value?.defaultEffort ?? null
  return info.value?.efforts.find((e) => e.effort === chosen)?.args ?? ''
})
const permissionArgs = computed(() => {
  const chosen = permission.value ?? info.value?.defaultPermission ?? null
  return info.value?.permissions.find((p) => p.permission === chosen)?.args ?? ''
})

// opencode applies an effort only to a model named beside it (LESSONS L5); the back end refuses it
// otherwise, and the form says so first.
const effortWithoutModel = computed(
  () => !!info.value?.effortNeedsModel && (effort.value ?? info.value.defaultEffort) !== null && model.value.trim() === '',
)
const carrier = computed(() => (info.value?.optionsInConfig ? 'server config: ' : ''))

const canSubmit = computed(
  () =>
    cwd.value.trim() !== '' &&
    (keyId.value === null || envVar.value.trim() !== '') &&
    !effortWithoutModel.value &&
    (providerId.value === null || model.value.trim() !== ''),
)

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
    providerId: providerId.value,
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
        <code v-if="effortArgs" class="args">{{ carrier }}{{ effortArgs }}</code>
        <span v-if="effortWithoutModel" class="hint warn">{{ info.displayName }} applies an effort only to a model named below.</span>
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
        <code v-if="permissionArgs" class="args">{{ carrier }}{{ permissionArgs }}</code>
      </label>
      <p v-else class="hint">{{ info.displayName }} has no permission mode Chorus can pass at launch yet.</p>

      <label v-if="routes.length">
        <span>Provider</span>
        <select v-model="providerId">
          <option :value="null">None: the agent's own login, or a key below</option>
          <option v-for="r in routes" :key="r.provider.id" :value="r.provider.id" :disabled="r.reason !== null">
            {{ r.provider.name }}{{ r.reason ? ` (not for ${info.displayName}: ${r.reason})` : '' }}
          </option>
        </select>
        <span v-if="keyNote" class="hint">{{ keyNote }}</span>
      </label>

      <label v-if="info.takesModel">
        <span>
          Model
          <em>{{ provider ? `(required: a ${provider.name} model id)` : "(optional: the CLI's own default otherwise)" }}</em>
        </span>
        <input
          v-model="model"
          maxlength="200"
          spellcheck="false"
          autocomplete="off"
          list="provider-shortlist"
          :placeholder="provider ? (provider.shortlist[0] ?? 'model id') : info.optionsInConfig ? 'provider/model, for example opencode/big-pickle' : ''"
        />
        <datalist id="provider-shortlist">
          <option v-for="m in provider?.shortlist ?? []" :key="m" :value="m" />
        </datalist>
        <span v-if="provider && !provider.shortlist.length" class="hint">Its shortlist is empty: pick models under Providers.</span>
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
