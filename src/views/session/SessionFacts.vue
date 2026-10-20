<script setup lang="ts">
/**
 * The session facts panel (glacier-session-raw.png, "SESSION"): project and tags (changeable),
 * agent, options, provider, key, conversation, CLI version, worktree and folder. Anything degraded
 * says why.
 */
import { computed, onMounted, ref } from 'vue'
import { commands, type SessionSummary } from '../../ipc'
import TagInput from '../../components/TagInput.vue'
import { useProjectsStore } from '../../stores/projects'
import { useSessionsStore } from '../../stores/sessions'
import { useAgentsStore } from '../../stores/agents'
import { AGENT_LABELS, money, observedText, optionsText, versionText } from '../../raw/labels'

const props = defineProps<{ session: SessionSummary }>()
const projects = useProjectsStore()
const sessions = useSessionsStore()
const agents = useAgentsStore()

const s = computed(() => props.session)
const projectChoices = computed(() => projects.list.filter((p) => p.state !== 'archived' || p.id === s.value.projectId))

function started(secs: number | null): string {
  return secs ? new Date(secs * 1000).toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' }) : '—'
}

const capped = computed(() => {
  const k = s.value.cappedKey
  if (!k) return null
  const state = k.live ? (s.value.live ? 'live' : 'not revoked yet') : 'revoked'
  return `capped at ${money(k.limitUsd)}, ${state}${k.costUsd === null ? '' : `, cost ${money(k.costUsd)}`}${k.note ? ` (${k.note})` : ''}`
})

async function move(value: string): Promise<void> {
  await sessions.setProject(s.value.id, value === '' ? null : Number(value))
}

const knownTags = ref<string[]>([])
onMounted(async () => {
  const r = await commands.tagsKnown().catch(() => null)
  if (r?.status === 'ok') knownTags.value = r.data.map((t) => t.tag)
})
async function retag(tags: string[]): Promise<void> {
  await sessions.setTags(s.value.id, tags)
}
</script>

<template>
  <section class="facts">
    <h3>Session</h3>
    <dl>
      <dt>Project</dt>
      <dd>
        <select :value="s.projectId ?? ''" aria-label="Project" @change="move(($event.target as HTMLSelectElement).value)">
          <option value="">No project</option>
          <option v-for="p in projectChoices" :key="p.id" :value="p.id">{{ p.name }}{{ p.state === 'archived' ? ' (archived)' : '' }}</option>
        </select>
      </dd>
      <dt>Tags</dt>
      <dd class="tags"><TagInput :model-value="s.tags" :known="knownTags" @update:model-value="retag" /></dd>
      <dt>Agent</dt>
      <dd>{{ AGENT_LABELS[s.agent] }}</dd>
      <template v-if="s.agent !== 'shell'">
        <dt>Options</dt>
        <dd>{{ optionsText(s) || 'the agent\'s defaults' }}</dd>
        <template v-if="observedText(s)">
          <dt>Last turn ran</dt>
          <dd :class="{ warn: s.observedDiffers.length }">
            {{ observedText(s) }}<template v-if="s.observedDiffers.length">, not what was asked: {{ s.observedDiffers.join('; ') }}</template>
          </dd>
        </template>
        <dt>Runs on</dt>
        <dd>{{ s.providerName ?? 'its own sign-in' }}</dd>
        <template v-if="capped">
          <dt>Run key</dt>
          <dd>{{ capped }}</dd>
        </template>
        <dt>CLI</dt>
        <dd :class="{ warn: s.versionFit === 'newer' || s.versionFit === 'older' || s.versionFit === 'unreadable' }">
          {{ versionText(s.cliVersion, s.versionFit, agents.byKind.get(s.agent)?.verifiedVersion ?? null) }}
        </dd>
        <template v-if="s.agentSessionId">
          <dt>Conversation</dt>
          <dd><code :title="s.agentSessionId">{{ s.agentSessionId.slice(0, 8) }}</code>{{ s.resumed ? ' · resumed' : '' }}</dd>
        </template>
      </template>
      <template v-if="s.credentialLabel">
        <dt>Key</dt>
        <dd>{{ s.credentialLabel }} as <code>{{ s.credentialEnv }}</code></dd>
      </template>
      <dt>Started</dt>
      <dd>{{ started(s.runStartedAtSecs ?? s.createdAtSecs) }}</dd>
      <template v-if="s.pid">
        <dt>Process</dt>
        <dd>pid {{ s.pid }}</dd>
      </template>
      <template v-if="s.worktree">
        <dt>Worktree</dt>
        <dd :title="s.worktree.path">
          <code>{{ s.worktree.branch }}</code>
          <span class="sub">{{ s.worktree.removed ? 'removed; the branch is kept' : `from ${s.worktree.baseBranch ?? 'a detached HEAD'}` }}</span>
        </dd>
      </template>
      <dt>Folder</dt>
      <dd><code class="path" :title="s.cwd">{{ s.cwd }}</code></dd>
    </dl>
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
dl {
  margin: 0;
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: 8px 14px;
  font-size: 13px;
}
dt {
  color: var(--text-muted);
}
dd {
  margin: 0;
  text-align: right;
  overflow-wrap: anywhere;
}
dd.warn {
  color: var(--status-needs);
}
select {
  max-width: 100%;
  padding: 3px 6px;
  font-size: 12.5px;
}
code {
  font-size: 12px;
}
.path {
  display: block;
  font-size: 11.5px;
  color: var(--text-2);
}
.sub {
  display: block;
  font-size: 11.5px;
  color: var(--text-muted);
}
dd.tags {
  text-align: left;
}
dd.tags :deep(.tags) {
  min-height: 30px;
  padding: 2px 4px;
  font-size: 12.5px;
}
</style>
