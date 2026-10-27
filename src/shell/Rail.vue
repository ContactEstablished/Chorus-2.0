<script setup lang="ts">
/**
 * The rail (DESIGN.md "Rail"): the mark and wordmark, New project and New session at the top, then
 * the destinations and the projects. A project's menu (right-click, or its ⋯) edits, hides, moves
 * and deletes it; dragging a project reorders them. The user can collapse it to icons; narrow
 * windows start collapsed.
 */
import { computed, nextTick, onUnmounted, ref } from 'vue'
import ChorusMark from '../components/ChorusMark.vue'
import Icon from '../components/Icon.vue'
import type { IconName } from '../components/icons'
import { useSessionsStore } from '../stores/sessions'
import { useProjectsStore } from '../stores/projects'
import { useViewStore, type ViewName } from '../stores/view'
import { useLayoutStore } from '../stores/layout'
import { counts, needsYou } from './status'
import { moveTo } from './projectOrder'

const emit = defineEmits<{ newProject: []; editProject: [id: number]; deleteProject: [id: number] }>()

const view = useViewStore()
const layout = useLayoutStore()
const sessions = useSessionsStore()
const projects = useProjectsStore()

const NAV: { name: ViewName; label: string; icon: IconName }[] = [
  { name: 'overview', label: 'Overview', icon: 'overview' },
  { name: 'sessions', label: 'Sessions', icon: 'sessions' },
  { name: 'bots', label: 'Bots', icon: 'bots' },
  { name: 'automations', label: 'Automations', icon: 'automations' },
  { name: 'loop', label: 'Loop', icon: 'loop' },
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

// Hidden projects stay out of the rail until asked for, so there is a way back to them.
const showHidden = ref(false)
const hiddenCount = computed(() => projects.list.filter((p) => p.state === 'hidden').length)
const shown = computed(() => projects.list.filter((p) => p.state === 'active' || (showHidden.value && p.state === 'hidden')))

// Reordering: the dragged row takes the place of the row it's dropped on.
const dragging = ref<number | null>(null)
const over = ref<number | null>(null)
const overEdge = computed(() => {
  if (dragging.value === null || over.value === null || dragging.value === over.value) return null
  const ids = shown.value.map((p) => p.id)
  return ids.indexOf(dragging.value) < ids.indexOf(over.value) ? 'after' : 'before'
})
function dragStart(e: DragEvent, id: number): void {
  dragging.value = id
  if (e.dataTransfer) {
    e.dataTransfer.effectAllowed = 'move'
    e.dataTransfer.setData('text/plain', String(id))
  }
}
function dragEnd(): void {
  dragging.value = null
  over.value = null
}
async function drop(target: number): Promise<void> {
  const from = dragging.value
  dragEnd()
  if (from === null || from === target) return
  await projects.reorder(moveTo(projects.list.map((p) => p.id), from, target))
}

// The project menu.
const menu = ref<{ id: number; x: number; y: number } | null>(null)
const menuEl = ref<HTMLElement | null>(null)
const menuProject = computed(() => (menu.value ? projects.byId.get(menu.value.id) ?? null : null))
const menuIndex = computed(() => shown.value.findIndex((p) => p.id === menu.value?.id))
const MENU_W = 200
const MENU_H = 176
async function openMenu(x: number, y: number, id: number): Promise<void> {
  menu.value = {
    id,
    x: Math.max(8, Math.min(x, window.innerWidth - MENU_W - 8)),
    y: Math.max(8, Math.min(y, window.innerHeight - MENU_H - 8)),
  }
  window.addEventListener('pointerdown', onOutside, true)
  await nextTick()
  menuEl.value?.querySelector<HTMLElement>('button:not(:disabled)')?.focus()
}
function closeMenu(): void {
  menu.value = null
  window.removeEventListener('pointerdown', onOutside, true)
}
function onOutside(e: PointerEvent): void {
  if (!menuEl.value?.contains(e.target as Node)) closeMenu()
}
function openMenuAt(e: MouseEvent, id: number): void {
  void openMenu(e.clientX, e.clientY, id)
}
function openMenuFrom(e: MouseEvent, id: number): void {
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect()
  void openMenu(r.left, r.bottom + 4, id)
}
onUnmounted(() => window.removeEventListener('pointerdown', onOutside, true))

function menuEdit(): void {
  const id = menu.value?.id
  closeMenu()
  if (id !== undefined) emit('editProject', id)
}
function menuDelete(): void {
  const id = menu.value?.id
  closeMenu()
  if (id !== undefined) emit('deleteProject', id)
}
async function menuHide(): Promise<void> {
  const p = menuProject.value
  closeMenu()
  if (!p) return
  const hiding = p.state === 'active'
  if ((await projects.setState(p.id, hiding ? 'hidden' : 'active')) && hiding && view.projectFilter === p.id) {
    view.openProject(null)
  }
}
async function menuMove(by: -1 | 1): Promise<void> {
  const id = menu.value?.id
  const neighbour = shown.value[menuIndex.value + by]
  closeMenu()
  if (id === undefined || !neighbour) return
  await projects.reorder(moveTo(projects.list.map((p) => p.id), id, neighbour.id))
}
function onMenuKey(e: KeyboardEvent): void {
  if (e.key === 'Escape') {
    e.stopPropagation()
    closeMenu()
  }
}
</script>

<template>
  <nav id="main-navigation" class="rail" :class="{ collapsed: layout.railCollapsed }" aria-label="Main">
    <div class="brand" data-tauri-drag-region>
      <ChorusMark :height="22" />
      <span class="wordmark" data-tauri-drag-region>Chorus</span>
      <button
        class="rail-toggle quiet"
        :title="layout.railCollapsed ? 'Expand sidebar' : 'Collapse sidebar'"
        :aria-label="layout.railCollapsed ? 'Expand sidebar' : 'Collapse sidebar'"
        :aria-expanded="!layout.railCollapsed"
        aria-controls="main-navigation"
        @click="layout.railCollapsed = !layout.railCollapsed"
      ><Icon :name="layout.railCollapsed ? 'chevronRight' : 'panelLeft'" :size="17" /></button>
    </div>

    <div class="actions">
      <button class="new quiet-outline" title="New project" aria-label="New project" @click="emit('newProject')">
        <Icon name="plus" :size="16" />
        <span class="label">New project</span>
      </button>
      <button class="new primary-outline" title="New session" aria-label="New session" @click="view.newSessionOpen = true">
        <Icon name="plus" :size="16" />
        <span class="label">New session</span>
      </button>
    </div>

    <ul class="nav">
      <li v-for="item in NAV" :key="item.name">
        <button class="item" :class="{ on: isOn(item.name) }" :title="item.label" :aria-label="item.label" @click="go(item.name)">
          <Icon :name="item.icon" />
          <span class="label">{{ item.label }}</span>
          <span v-if="item.name === 'overview' && needs" class="badge" :title="`${needs} need you`">{{ needs }}</span>
        </button>
      </li>
    </ul>

    <div class="projects-head">
      <span class="label">Projects</span>
    </div>
    <ul class="projects scroll">
      <li
        v-for="p in shown"
        :key="p.id"
        class="prow"
        :class="{
          dim: p.state !== 'active',
          dragging: dragging === p.id,
          'drop-before': over === p.id && overEdge === 'before',
          'drop-after': over === p.id && overEdge === 'after',
        }"
        draggable="true"
        @dragstart="dragStart($event, p.id)"
        @dragend="dragEnd"
        @dragover.prevent="over = p.id"
        @drop.prevent="drop(p.id)"
        @contextmenu.prevent="openMenuAt($event, p.id)"
      >
        <button
          class="item project"
          :class="{ on: view.name === 'overview' && view.projectFilter === p.id }"
          :title="p.folder ? `${p.name} — ${p.folder}` : p.name"
          :aria-label="p.state === 'hidden' ? `${p.name} (hidden)` : p.name"
          @click="view.openProject(p.id)"
        >
          <span class="swatch" :style="{ background: p.colour }" />
          <span class="label">{{ p.name }}</span>
          <span class="count">{{ perProject.get(p.id)?.count || '' }}</span>
          <span class="attn" :class="{ on: perProject.get(p.id)?.attention }" />
        </button>
        <button
          class="more quiet"
          :title="`${p.name} options`"
          :aria-label="`${p.name} options`"
          aria-haspopup="menu"
          @click.stop="openMenuFrom($event, p.id)"
        ><Icon name="more" :size="15" /></button>
      </li>
      <li v-if="!shown.length && !hiddenCount" class="empty label">
        <button class="quiet link" @click="emit('newProject')">Add a project</button>
      </li>
    </ul>
    <button
      v-if="hiddenCount"
      class="hidden-toggle quiet"
      :aria-pressed="showHidden"
      :title="showHidden ? 'Hide the hidden projects again' : 'Show the projects you hid'"
      @click="showHidden = !showHidden"
    >
      <Icon name="eyeOff" :size="14" />
      <span class="label">{{ showHidden ? 'Hide' : 'Show' }} {{ hiddenCount }} hidden</span>
    </button>

    <Teleport to="body">
      <div
        v-if="menu && menuProject"
        ref="menuEl"
        class="project-menu"
        role="menu"
        :aria-label="`${menuProject.name} options`"
        :style="{ left: `${menu.x}px`, top: `${menu.y}px` }"
        @keydown="onMenuKey"
        @contextmenu.prevent
      >
        <button role="menuitem" @click="menuEdit">Edit project…</button>
        <button role="menuitem" @click="menuHide">{{ menuProject.state === 'active' ? 'Hide from sidebar' : 'Show in sidebar' }}</button>
        <button role="menuitem" :disabled="menuIndex <= 0" @click="menuMove(-1)">Move up</button>
        <button role="menuitem" :disabled="menuIndex < 0 || menuIndex >= shown.length - 1" @click="menuMove(1)">Move down</button>
        <hr />
        <button role="menuitem" class="danger-item" @click="menuDelete">Delete project…</button>
      </div>
    </Teleport>

  </nav>
</template>

<style scoped>
.rail {
  width: var(--rail-w);
  flex: none;
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
.rail-toggle { margin-left: auto; padding: 5px; display: grid; place-items: center; }
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
.prow {
  position: relative;
  border-radius: var(--radius-control);
}
.prow.dim .project {
  opacity: 0.6;
}
.prow.dragging {
  opacity: 0.45;
}
.prow.drop-before::before,
.prow.drop-after::after {
  content: '';
  position: absolute;
  left: 6px;
  right: 6px;
  height: 2px;
  border-radius: 1px;
  background: var(--accent);
}
.prow.drop-before::before { top: -2px; }
.prow.drop-after::after { bottom: -2px; }
.project {
  padding: 7px 10px;
}
.more {
  position: absolute;
  top: 50%;
  right: 4px;
  transform: translateY(-50%);
  padding: 3px 4px;
  display: none;
  place-items: center;
  color: var(--text-muted);
}
.prow:hover .more,
.prow:focus-within .more {
  display: grid;
}
.prow:hover .count,
.prow:hover .attn,
.prow:focus-within .count,
.prow:focus-within .attn {
  visibility: hidden;
}
.hidden-toggle {
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 6px;
  padding: 5px 10px;
  color: var(--text-muted);
  font-size: 12px;
  text-align: left;
}
.hidden-toggle:hover:not(:disabled) {
  color: var(--text);
}
.project-menu {
  position: fixed;
  z-index: 60;
  width: 200px;
  padding: 5px;
  display: flex;
  flex-direction: column;
  border-radius: 10px;
  background: var(--surface-raised);
  border: 1px solid var(--border-strong);
  box-shadow: 0 10px 30px rgb(0 0 0 / 0.4);
}
.project-menu button {
  padding: 7px 10px;
  border: 1px solid transparent;
  border-radius: 7px;
  background: transparent;
  color: var(--text);
  font-size: 13px;
  text-align: left;
}
.project-menu button:hover:not(:disabled),
.project-menu button:focus-visible {
  background: var(--surface-chip);
}
.project-menu button:disabled {
  color: var(--text-muted);
  opacity: 0.55;
}
.project-menu hr {
  width: 100%;
  margin: 4px 0;
  border: 0;
  border-top: 1px solid var(--border);
}
.project-menu .danger-item {
  color: var(--status-failed);
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
.actions { display: flex; flex-direction: column; gap: 6px; margin-bottom: 16px; flex: none; }
.new {
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
.new.quiet-outline {
  background: transparent;
  border-color: var(--border-strong);
  color: var(--text-2);
}
.new.quiet-outline:hover:not(:disabled) {
  color: var(--text);
  border-color: var(--text-muted);
}
.new .label {
  flex: none;
}

  .rail.collapsed {
    width: 60px;
    padding: 0 8px 12px;
  }
  .collapsed .brand {
    justify-content: center;
    padding: 0;
    flex-direction: column;
    height: 88px;
    gap: 8px;
  }
  .collapsed .rail-toggle { margin-left: 0; }
  .collapsed .wordmark,
  .collapsed .label,
  .collapsed .count {
    display: none;
  }
  .collapsed .item {
    justify-content: center;
    padding: 9px 0;
    position: relative;
  }
  .collapsed .badge {
    position: absolute;
    top: 1px;
    right: 1px;
    min-width: 0;
    padding: 0 4px;
    font-size: 10px;
  }
  .collapsed .projects-head {
    justify-content: center;
    padding: 0;
  }
  .collapsed .attn {
    position: absolute;
    top: 6px;
    right: 8px;
  }
  .collapsed .swatch {
    width: 12px;
    height: 12px;
  }
  .collapsed .empty,
  .collapsed .more {
    display: none !important;
  }
  .collapsed .hidden-toggle {
    justify-content: center;
    padding: 6px 0;
  }
  .collapsed .new {
    padding: 9px 0;
  }
</style>
