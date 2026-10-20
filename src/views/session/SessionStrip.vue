<script setup lang="ts">
/**
 * The session strip (D26, the filmstrip's job): every session as a small chip, most urgent first,
 * so switching is one click. Collapsible; the choice is kept in preferences.
 */
import { computed } from 'vue'
import Icon from '../../components/Icon.vue'
import { useSessionsStore } from '../../stores/sessions'
import { useProjectsStore } from '../../stores/projects'
import { useViewStore } from '../../stores/view'
import { usePrefsStore } from '../../stores/prefs'
import { AGENT_CHIPS, ATTENTION, byUrgency } from '../../shell/status'

defineProps<{ current: number }>()

const sessions = useSessionsStore()
const projects = useProjectsStore()
const view = useViewStore()
const prefs = usePrefsStore()

const ordered = computed(() => [...sessions.list].sort(byUrgency))
</script>

<template>
  <div class="strip" :class="{ collapsed: !prefs.prefs.sessionStrip }">
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
        <span class="agent-chip" :class="`agent-${s.agent}`">{{ AGENT_CHIPS[s.agent] }}</span>
        <span class="name">{{ s.name }}</span>
        <span
          v-if="s.projectId !== null && projects.byId.get(s.projectId)"
          class="sq"
          :style="{ background: projects.byId.get(s.projectId)!.colour }"
        />
      </button>
    </template>
    <span v-else class="label">{{ sessions.list.length }} sessions</span>
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
  height: 17px;
  min-width: 22px;
  font-size: 9.5px;
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
