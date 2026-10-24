<script setup lang="ts">
/**
 * One session on the Overview (glacier-overview.png): agent, status, name, project and folder, what
 * it last said, and how long it has run. The whole card opens the session.
 */
import { computed, ref } from 'vue'
import { commands, type Project, type SessionSummary } from '../ipc'
import Icon from '../components/Icon.vue'
import { compactPath } from '../shell/compactPath'
import { useNoticesStore } from '../stores/notices'
import { AGENT_LABELS } from '../raw/labels'
import AgentLogo from '../components/AgentLogo.vue'
import { ATTENTION, ago, shortDuration, shortPath } from '../shell/status'

const props = defineProps<{ session: SessionSummary; project: Project | null; home: string | null; now: number }>()
defineEmits<{ open: [] }>()

const s = computed(() => props.session)
const notices = useNoticesStore()
const openingFolder = ref(false)
async function openFolder(): Promise<void> {
  if (openingFolder.value) return
  openingFolder.value = true
  try {
    const result = await commands.sessionOpenFolder(s.value.id)
    if (result.status === 'error') notices.error('Could not open the session folder.', result.error)
  } catch (e) {
    notices.error('Could not open the session folder. Restart Chorus if its backend has not been updated yet.', e)
  } finally { openingFolder.value = false }
}
const label = computed(() => (s.value.attention === 'working' && s.value.noSignal ? 'Working · no signal' : ATTENTION[s.value.attention].label))

/** What it last said or was asked; for a failure, why it stopped comes first. */
const body = computed(() => {
  const x = s.value
  if (x.attention === 'failed' && x.note) return x.note
  return x.latest ?? x.description ?? x.note ?? (x.agent === 'shell' ? 'A shell. Chorus records no turns for it.' : 'No turns yet.')
})

const timing = computed(() => {
  const x = s.value
  const up = x.runStartedAtSecs ? props.now - x.runStartedAtSecs : 0
  const ran = !x.live ? 'stopped' : up < 60 ? 'just started' : shortDuration(up)
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
    :aria-label="`${s.name}: ${AGENT_LABELS[s.agent]}, ${label}`"
    @click="$emit('open')"
    @keydown.enter.self.prevent="$emit('open')"
    @keydown.space.self.prevent="$emit('open')"
  >
    <header class="top">
      <AgentLogo :agent="s.agent" />
      <h3 class="name">
        <span class="name-text" :title="s.name">{{ s.name }}</span>
        <span v-if="s.locked" class="locked" title="Locked: Close and Restart are off until it's unlocked">locked</span>
      </h3>
      <span class="status"><span class="dot" :class="{ pulse: s.attention === 'working' && !s.noSignal }" />{{ label }}</span>
    </header>
    <p class="where">
      <span v-if="s.providerName" class="agent">{{ s.providerName }}</span>
      <span v-if="project" class="project-tag" :style="{ '--tag': project.colour }">{{ project.name }}</span>
      <span v-for="t in s.tags" :key="t" class="tag">{{ t }}</span>
      <span class="folder">
        <button
          class="folder-link quiet"
          type="button"
          :disabled="openingFolder"
          :title="`Open folder in File Explorer: ${s.cwd}`"
          :aria-label="`Open ${s.name}'s folder in File Explorer`"
          @click.stop="openFolder"
          @keydown.stop
        ><Icon name="folder" :size="15" /></button>
        <code :title="s.worktree ? `${s.cwd}\nworktree on ${s.worktree.branch}` : s.cwd">{{ compactPath(shortPath(s.cwd, home)) }}</code>
      </span>
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
  font-size: 12px;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.status {
  margin-left: auto;
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
  flex: 1;
  margin: 0;
  font-size: 17px;
  font-weight: 650;
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.name-text {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.locked {
  flex: none;
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
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  min-width: 0;
}
.tag {
  padding: 1px 8px;
  border-radius: var(--radius-pill);
  border: 1px solid var(--border-strong);
  font-size: 11.5px;
  color: var(--text-2);
  white-space: nowrap;
}
.folder {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  max-width: 100%;
}
.folder-link {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: none;
  width: 22px;
  height: 22px;
  padding: 0;
  color: var(--text-muted);
}
.folder-link:hover { color: var(--accent); }
.folder code {
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
