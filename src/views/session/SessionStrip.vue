<script setup lang="ts">
/**
 * The session strip (D26, the filmstrip's job): the open session's project's sessions as small chips,
 * most urgent first, so switching is one click. With only one session there is nothing to switch to,
 * so the strip takes no room. Collapsible; the choice is kept in preferences.
 */
import { computed } from 'vue'
import Icon from '../../components/Icon.vue'
import { useSessionsStore } from '../../stores/sessions'
import { useProjectsStore } from '../../stores/projects'
import { useViewStore } from '../../stores/view'
import { usePrefsStore } from '../../stores/prefs'
import { ATTENTION } from '../../shell/status'
import { AGENT_LOGOS } from '../../components/agentLogos'
import { AGENT_LABELS } from '../../raw/labels'
import { stripSessions } from './stripSessions'

const props = defineProps<{ current: number }>()

const sessions = useSessionsStore()
const projects = useProjectsStore()
const view = useViewStore()
const prefs = usePrefsStore()

const ordered = computed(() => stripSessions(sessions.list, props.current))
</script>

<template>
  <div v-if="ordered.length > 1" class="strip" :class="{ collapsed: !prefs.prefs.sessionStrip }">
    <template v-if="prefs.prefs.sessionStrip">
      <button
        v-for="s in ordered"
        :key="s.id"
        class="chip"
        :class="[`st-${s.attention}`, { on: s.id === current }]"
        :title="`${s.name} — ${ATTENTION[s.attention].label}`"
        :aria-current="s.id === current ? 'page' : undefined"
        @click="view.openSession(s.id)"
      >
        <span class="dot" :class="{ pulse: s.attention === 'working' && !s.noSignal }" />
        <span class="agent-chip" :class="`agent-${s.agent}`" :title="AGENT_LABELS[s.agent]"><img :src="AGENT_LOGOS[s.agent]" :alt="AGENT_LABELS[s.agent]" /></span>
        <span class="name">{{ s.name }}</span>
        <span
          v-if="s.projectId !== null && projects.byId.get(s.projectId)"
          class="sq"
          :style="{ background: projects.byId.get(s.projectId)!.colour }"
        />
      </button>
    </template>
    <span v-else class="label">{{ ordered.length }} sessions</span>
    <button
      class="toggle"
      :title="prefs.prefs.sessionStrip ? 'Hide the session strip' : 'Show the session strip'"
      :aria-label="prefs.prefs.sessionStrip ? 'Hide the session strip' : 'Show the session strip'"
      @click="prefs.update({ sessionStrip: !prefs.prefs.sessionStrip })"
    >
      <Icon name="chevronDown" :size="14" :style="{ transform: prefs.prefs.sessionStrip ? 'rotate(180deg)' : '' }" />
    </button>
  </div>
</template>

<style scoped>
.strip {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 16px;
  border-bottom: 1px solid var(--border);
  overflow-x: auto;
  scrollbar-width: thin;
  scrollbar-color: var(--border-strong) transparent;
  flex: none;
}
.strip.collapsed {
  padding-top: 4px;
  padding-bottom: 4px;
}
.chip {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 4px 10px 4px 8px;
  border-radius: 9px;
  background: var(--surface);
  border: 1px solid var(--border);
  font-size: 12.5px;
  white-space: nowrap;
  flex: none;
  max-width: 220px;
}
.chip.on {
  border-color: var(--accent-line);
  background: var(--accent-soft);
}
.chip .agent-chip {
  height: 20px;
  width: 20px;
  min-width: 20px;
  padding: 0;
}
.chip .agent-chip img {
  width: 15px;
  height: 15px;
  object-fit: contain;
}
.name {
  overflow: hidden;
  text-overflow: ellipsis;
}
.sq {
  width: 7px;
  height: 7px;
  border-radius: 2px;
  flex: none;
}
.label {
  color: var(--text-muted);
  font-size: 12px;
}
.toggle {
  margin-left: auto;
  padding: 3px;
  border: 0;
  background: transparent;
  color: var(--text-muted);
  display: grid;
  flex: none;
  position: sticky;
  right: 0;
}
.toggle:hover:not(:disabled) {
  color: var(--text);
}
</style>
