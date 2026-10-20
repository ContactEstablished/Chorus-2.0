<script setup lang="ts">
/**
 * One session on the Overview (glacier-overview.png): agent, status, name, project and folder, what
 * it last said, and how long it has run. The whole card opens the session.
 */
import { computed } from 'vue'
import type { Project, SessionSummary } from '../ipc'
import { AGENT_LABELS } from '../raw/labels'
import { AGENT_CHIPS, ATTENTION, ago, shortDuration, shortPath } from '../shell/status'

const props = defineProps<{ session: SessionSummary; project: Project | null; home: string | null; now: number }>()
defineEmits<{ open: [] }>()

const s = computed(() => props.session)
const label = computed(() => (s.value.attention === 'working' && s.value.noSignal ? 'Working · no signal' : ATTENTION[s.value.attention].label))

/** What it last said or was asked; for a failure, why it stopped comes first. */
const body = computed(() => {
  const x = s.value
  if (x.attention === 'failed' && x.note) return x.note
  return x.latest ?? x.description ?? x.note ?? (x.agent === 'shell' ? 'A shell. Chorus records no turns for it.' : 'No turns yet.')
})

const timing = computed(() => {
  const x = s.value
  const ran = x.live && x.runStartedAtSecs ? shortDuration(props.now - x.runStartedAtSecs) : x.live ? 'starting' : 'stopped'
  const updated = x.latestAtSecs || x.runStartedAtSecs || x.createdAtSecs
  return `${ran} · updated ${ago(updated, props.now)}`
})
</script>

<template>
  <article
    class="card"
    :class="`st-${s.attention}`"
    tabindex="0"
    role="button"
    :aria-label="`${s.name}: ${label}`"
    @click="$emit('open')"
    @keydown.enter.prevent="$emit('open')"
    @keydown.space.prevent="$emit('open')"
  >
    <header class="top">
      <span class="agent-chip" :class="`agent-${s.agent}`">{{ AGENT_CHIPS[s.agent] }}</span>
      <span class="agent">{{ AGENT_LABELS[s.agent] }}<template v-if="s.providerName"> · {{ s.providerName }}</template></span>
      <span class="status"><span class="dot" :class="{ pulse: s.attention === 'working' && !s.noSignal }" />{{ label }}</span>
    </header>
    <h3 class="name">
      {{ s.name }}
      <span v-if="s.locked" class="locked" title="Locked: Close and Restart are off until it's unlocked">locked</span>
    </h3>
    <p class="where">
      <span v-if="project" class="project-tag" :style="{ '--tag': project.colour }">{{ project.name }}</span>
      <code :title="s.cwd">{{ shortPath(s.cwd, home) }}</code>
    </p>
    <p class="body">{{ body }}</p>
    <footer class="foot">
      <span>{{ timing }}</span>
      <span class="cta">{{ ATTENTION[s.attention].cta }} →</span>
    </footer>
  </article>
</template>

<style scoped>
.card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 16px 20px 14px;
  border-radius: var(--radius-card);
  background: var(--surface);
  border: 1px solid var(--border);
  border-top: 2px solid var(--st);
  cursor: pointer;
  min-width: 0;
  animation: rise 250ms ease both;
}
.card.st-done,
.card.st-running {
  border-top-color: var(--border-strong);
}
.card:hover {
  background: var(--surface-raised);
}
.card:focus-visible {
  outline: 2px solid var(--accent-line);
  outline-offset: 2px;
}
@keyframes rise {
  from {
    opacity: 0;
    transform: translateY(6px);
  }
}
.top {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  min-width: 0;
}
.agent {
  color: var(--text-2);
  flex: 1;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.status {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  color: var(--st);
  font-weight: 500;
  white-space: nowrap;
}
.card.st-done .status,
.card.st-running .status {
  color: var(--text-muted);
}
.name {
  margin: 0;
  font-size: 17px;
  font-weight: 650;
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  overflow-wrap: anywhere;
}
.locked {
  font-family: var(--font-ui);
  font-size: 10.5px;
  font-weight: 600;
  padding: 1px 7px;
  border-radius: var(--radius-pill);
  background: var(--surface-chip);
  color: var(--text-2);
}
.where {
  margin: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}
.where code {
  font-size: 12.5px;
  color: var(--text-muted);
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  min-width: 0;
}
.body {
  margin: 2px 0 4px;
  color: var(--text-2);
  font-size: 14px;
  line-height: 1.5;
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
  overflow-wrap: anywhere;
  white-space: pre-line;
  flex: 1;
}
.foot {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  padding-top: 12px;
  border-top: 1px solid var(--border);
  color: var(--text-muted);
  font-size: 12.5px;
}
.cta {
  color: var(--st);
  font-weight: 600;
  white-space: nowrap;
}
.card.st-done .cta,
.card.st-running .cta {
  color: var(--accent);
}
</style>
