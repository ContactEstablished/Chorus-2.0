<script setup lang="ts">
/**
 * The rail (DESIGN.md "Rail"): the mark and wordmark, the destinations, the projects, and New
 * session at the bottom. Under 720 px wide it shows icons only.
 */
import { computed } from 'vue'
import ChorusMark from '../components/ChorusMark.vue'
import Icon from '../components/Icon.vue'
import type { IconName } from '../components/icons'
import { useSessionsStore } from '../stores/sessions'
import { useProjectsStore } from '../stores/projects'
import { useViewStore, type ViewName } from '../stores/view'
import { counts, needsYou } from './status'

const emit = defineEmits<{ newProject: []; editProject: [id: number] }>()

const view = useViewStore()
const sessions = useSessionsStore()
const projects = useProjectsStore()

const NAV: { name: ViewName; label: string; icon: IconName }[] = [
  { name: 'overview', label: 'Overview', icon: 'overview' },
  { name: 'sessions', label: 'Sessions', icon: 'sessions' },
  { name: 'bots', label: 'Bots', icon: 'bots' },
  { name: 'automations', label: 'Automations', icon: 'automations' },
  { name: 'documents', label: 'Documents', icon: 'documents' },
  { name: 'notes', label: 'Notes', icon: 'notes' },
  { name: 'settings', label: 'Settings', icon: 'settings' },
]

const needs = computed(() => counts(sessions.list).needs)

/** Per project: how many sessions, and whether one needs you. */
const perProject = computed(() => {
  const m = new Map<number, { count: number; attention: boolean }>()
  for (const s of sessions.list) {
    if (s.projectId === null) continue
    const e = m.get(s.projectId) ?? { count: 0, attention: false }
    e.count++
    e.attention ||= needsYou(s.attention)
    m.set(s.projectId, e)
  }
  return m
})

function isOn(name: ViewName): boolean {
  if (name === 'sessions') return view.name === 'sessions' || view.name === 'session'
  if (name === 'overview') return view.name === 'overview' && view.projectFilter === null
  return view.name === name
}

function go(name: ViewName): void {
  if (name === 'overview') view.openProject(null)
  else view.go(name)
}
</script>

<template>
  <nav class="rail" aria-label="Main">
    <div class="brand" data-tauri-drag-region>
      <ChorusMark :height="22" />
      <span class="wordmark" data-tauri-drag-region>Chorus</span>
    </div>

    <ul class="nav">
      <li v-for="item in NAV" :key="item.name">
        <button class="item" :class="{ on: isOn(item.name) }" :title="item.label" @click="go(item.name)">
          <Icon :name="item.icon" />
          <span class="label">{{ item.label }}</span>
          <span v-if="item.name === 'overview' && needs" class="badge" :title="`${needs} need you`">{{ needs }}</span>
        </button>
      </li>
    </ul>

    <div class="projects-head">
      <span class="label">Projects</span>
      <button class="add" title="New project" aria-label="New project" @click="emit('newProject')"><Icon name="plus" :size="14" /></button>
    </div>
    <ul class="projects scroll">
      <li v-for="p in projects.active" :key="p.id">
        <button
          class="item project"
          :class="{ on: view.name === 'overview' && view.projectFilter === p.id }"
          :title="p.folder ? `${p.name} — ${p.folder}` : p.name"
          @click="view.openProject(p.id)"
          @contextmenu.prevent="emit('editProject', p.id)"
        >
          <span class="swatch" :style="{ background: p.colour }" />
          <span class="label">{{ p.name }}</span>
          <span class="count">{{ perProject.get(p.id)?.count || '' }}</span>
          <span class="attn" :class="{ on: perProject.get(p.id)?.attention }" />
        </button>
      </li>
      <li v-if="!projects.active.length" class="empty label">
        <button class="quiet link" @click="emit('newProject')">Add a project</button>
        <template v-if="projects.importAvailable">
          or <button class="quiet link" @click="view.openSettings('projects')">bring them from Chorus 1</button>
        </template>
      </li>
    </ul>

    <button class="new primary-outline" title="New session" @click="view.newSessionOpen = true">
      <Icon name="plus" :size="16" />
      <span class="label">New session</span>
    </button>
  </nav>
</template>

<style scoped>
.rail {
  width: var(--rail-w);
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-sidebar);
  border-right: 1px solid var(--border);
  padding: 0 10px 12px;
  min-height: 0;
}
.brand {
  height: 62px;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 8px;
  flex: none;
}
.wordmark {
  font-family: var(--font-heading);
  font-weight: 700;
  font-size: 17px;
  letter-spacing: -0.01em;
}
ul {
  list-style: none;
  margin: 0;
  padding: 0;
}
.nav {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.item {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 11px;
  padding: 8px 10px;
  border: 1px solid transparent;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-2);
  font-weight: 500;
  text-align: left;
}
.item:hover:not(:disabled) {
  color: var(--text);
  background: var(--surface);
  border-color: transparent;
}
.item.on {
  background: var(--surface-chip);
  color: var(--text);
  font-weight: 600;
}
.label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.badge {
  min-width: 20px;
  padding: 1px 6px;
  border-radius: 6px;
  background: color-mix(in srgb, var(--status-needs) 18%, transparent);
  color: var(--status-needs);
  font-size: 11.5px;
  font-weight: 700;
  text-align: center;
}
.projects-head {
  display: flex;
  align-items: center;
  margin: 26px 0 6px;
  padding: 0 10px;
  color: var(--text-muted);
  font-size: 11.5px;
  font-weight: 600;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}
.add {
  padding: 2px;
  border: 0;
  background: transparent;
  color: var(--text-muted);
  display: grid;
}
.add:hover:not(:disabled) {
  color: var(--text);
}
.projects {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.project {
  padding: 7px 10px;
}
.swatch {
  width: 9px;
  height: 9px;
  border-radius: 3px;
  margin: 0 2px;
  flex: none;
}
.count {
  color: var(--text-muted);
  font-size: 12px;
  min-width: 10px;
  text-align: right;
}
.attn {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex: none;
}
.attn.on {
  background: var(--status-needs);
}
.empty {
  padding: 4px 10px;
  color: var(--text-muted);
  font-size: 12.5px;
  white-space: normal;
  line-height: 1.6;
}
.link {
  padding: 0;
  color: var(--accent);
  font-size: inherit;
}
.link:hover:not(:disabled) {
  background: transparent;
  text-decoration: underline;
}
.new {
  margin-top: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 9px 12px;
  background: var(--accent-soft);
  border: 1px solid var(--accent-line);
  color: var(--accent);
  font-weight: 600;
}
.new:hover:not(:disabled) {
  border-color: var(--accent);
}
.new .label {
  flex: none;
}

@media (max-width: 720px) {
  .rail {
    width: 60px;
    padding: 0 8px 12px;
  }
  .brand {
    justify-content: center;
    padding: 0;
  }
  .wordmark,
  .label,
  .count,
  .projects-head .label {
    display: none;
  }
  .item {
    justify-content: center;
    padding: 9px 0;
    position: relative;
  }
  .badge {
    position: absolute;
    top: 1px;
    right: 1px;
    min-width: 0;
    padding: 0 4px;
    font-size: 10px;
  }
  .projects-head {
    justify-content: center;
    padding: 0;
  }
  .attn {
    position: absolute;
    top: 6px;
    right: 8px;
  }
  .swatch {
    width: 12px;
    height: 12px;
  }
  .empty {
    display: none;
  }
  .new {
    padding: 9px 0;
  }
}
</style>
