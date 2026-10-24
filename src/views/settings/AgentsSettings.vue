<script setup lang="ts">
/** The agent CLIs Chorus looked for: found or not, the version, and how it compares (fail visible). */
import { onMounted, ref } from 'vue'
import { commands, type AgentInfo, type AgentKind, type InstructionSupport } from '../../ipc'
import { useAgentsStore } from '../../stores/agents'
import { AGENT_CHIPS } from '../../shell/status'
import { versionText } from '../../raw/labels'

const agents = useAgentsStore()
// P8: how each agent takes Bot instructions and notes, or why it doesn't.
const instructions = ref(new Map<AgentKind, InstructionSupport>())
onMounted(async () => {
  void agents.load(false)
  instructions.value = new Map((await commands.instructionsSupport()).map((i) => [i.agent, i]))
})

function state(a: AgentInfo): { text: string; tone: 'ok' | 'warn' | 'bad' | 'muted' } {
  const d = a.detection
  if (!d) return { text: 'Looking…', tone: 'muted' }
  if (!d.found) return { text: d.problem ?? 'Not found', tone: 'bad' }
  if (d.problem) return { text: d.problem, tone: 'warn' }
  return { text: versionText(d.version, d.fit, a.verifiedVersion), tone: d.fit === 'verified' || d.fit === null ? 'ok' : 'warn' }
}
</script>

<template>
  <div>
    <div class="head">
      <div>
        <h2>Agents</h2>
        <p class="hint">Chorus runs the agent CLIs installed on this computer. Versions it hasn't checked are tried and reported, never refused.</p>
      </div>
      <button :disabled="agents.detecting" @click="agents.load(true)">{{ agents.detecting ? 'Looking…' : 'Look again' }}</button>
    </div>
    <p v-if="agents.error" class="error" role="alert">{{ agents.error }}</p>
    <ul class="list">
      <li v-for="a in agents.list" :key="a.kind" class="agent">
        <span class="agent-chip" :class="`agent-${a.kind}`">{{ AGENT_CHIPS[a.kind] }}</span>
        <div class="body">
          <p class="name">{{ a.displayName }}</p>
          <p class="state" :class="state(a).tone">{{ state(a).text }}</p>
          <p v-if="instructions.get(a.kind)" class="state" :class="instructions.get(a.kind)!.supported ? 'muted' : 'warn'">{{ instructions.get(a.kind)!.text }}</p>
          <code v-if="a.detection?.program" class="program" :title="a.detection.program">{{ a.detection.program }}</code>
        </div>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.head {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 16px;
}
h2 {
  margin: 0 0 6px;
  font-size: 17px;
}
.hint {
  margin: 0 0 16px;
  color: var(--text-2);
  font-size: 13px;
  max-width: 60ch;
}
.list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.agent {
  display: flex;
  gap: 12px;
  align-items: flex-start;
  padding: 12px 14px;
  border-radius: 12px;
  background: var(--surface);
  border: 1px solid var(--border);
}
.body {
  min-width: 0;
  flex: 1;
}
.name {
  margin: 0;
  font-weight: 600;
}
.state {
  margin: 3px 0 0;
  font-size: 13px;
}
.state.ok {
  color: var(--text-2);
}
.state.warn {
  color: var(--status-needs);
}
.state.bad {
  color: var(--status-failed);
}
.state.muted {
  color: var(--text-muted);
}
.program {
  display: block;
  margin-top: 4px;
  font-size: 11.5px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.error {
  color: var(--status-failed);
}
</style>
