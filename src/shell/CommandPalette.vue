<script setup lang="ts">
/**
 * The command palette (Ctrl+K): sessions, projects, places and actions, filtered as you type. In
 * projects mode (Ctrl+G) it lists projects only, to switch the Overview between them. Enter runs
 * the highlighted item; Esc closes. Typed, pasted or dictated text only filters: nothing runs
 * until Enter or a click.
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import Icon from '../components/Icon.vue'
import { useSessionsStore } from '../stores/sessions'
import { useProjectsStore } from '../stores/projects'
import { usePrefsStore } from '../stores/prefs'
import { useViewStore } from '../stores/view'
import { AGENT_LABELS } from '../raw/labels'
import { AGENT_CHIPS, ATTENTION, byUrgency, needsYou } from './status'
import { filterItems, moveSelection, type PaletteItem } from './palette'

const props = defineProps<{ mode: 'all' | 'projects' }>()

const sessions = useSessionsStore()
const projects = useProjectsStore()
const prefs = usePrefsStore()
const view = useViewStore()

const query = ref('')
const selected = ref(0)
const input = ref<HTMLInputElement | null>(null)
const listEl = ref<HTMLElement | null>(null)

function close(): void {
  view.palette = null
}

const sessionItems = computed<PaletteItem[]>(() =>
  [...sessions.list].sort(byUrgency).map((s) => {
    const p = s.projectId === null ? null : projects.byId.get(s.projectId)
    return {
      id: `s${s.id}`,
      group: 'Sessions',
      label: s.name,
      hint: [AGENT_LABELS[s.agent], ATTENTION[s.attention].label, p?.name].filter(Boolean).join(' · '),
      keywords: `${s.cwd} ${s.description ?? ''} ${AGENT_CHIPS[s.agent]} ${s.tags.join(' ')} ${s.worktree?.branch ?? ''}`,
      run: () => view.openSession(s.id),
    }
  }),
)

const projectItems = computed<PaletteItem[]>(() => [
  { id: 'p-all', group: 'Projects', label: 'All projects', hint: `${sessions.list.length} sessions`, run: () => view.openProject(null) },
  ...projects.usable.map((p) => {
    const mine = sessions.list.filter((s) => s.projectId === p.id)
    const waiting = mine.filter((s) => needsYou(s.attention)).length
    return {
      id: `p${p.id}`,
      group: 'Projects',
      label: p.name,
      hint: [`${mine.length} session${mine.length === 1 ? '' : 's'}`, waiting ? `${waiting} need${waiting === 1 ? 's' : ''} you` : null, p.state === 'hidden' ? 'hidden' : null]
        .filter(Boolean)
        .join(' · '),
      keywords: p.folder ?? '',
      run: () => view.openProject(p.id),
    }
  }),
])

const placeItems = computed<PaletteItem[]>(() => [
  { id: 'go-overview', group: 'Go to', label: 'Overview', run: () => view.openProject(null) },
  { id: 'go-sessions', group: 'Go to', label: 'Sessions', run: () => ((view.sessionsLayout = 'list'), view.go('sessions')) },
  { id: 'go-wall', group: 'Go to', label: 'Wall of live terminals', keywords: 'sessions grid', run: () => ((view.sessionsLayout = 'wall'), view.go('sessions')) },
  { id: 'go-appearance', group: 'Go to', label: 'Settings: Appearance', keywords: 'theme zoom', run: () => view.openSettings('appearance') },
  { id: 'go-projects', group: 'Go to', label: 'Settings: Projects', keywords: 'colours folders', run: () => view.openSettings('projects') },
  { id: 'go-providers', group: 'Go to', label: 'Settings: Providers', keywords: 'openrouter jev gateway models', run: () => view.openSettings('providers') },
  { id: 'go-keys', group: 'Go to', label: 'Settings: Keys and lock', keywords: 'api key pin vault', run: () => view.openSettings('keys') },
  { id: 'go-agents', group: 'Go to', label: 'Settings: Agents', keywords: 'cli version', run: () => view.openSettings('agents') },
])

const actionItems = computed<PaletteItem[]>(() => {
  const p = prefs.prefs
  return [
    { id: 'a-new', group: 'Actions', label: 'New session', keywords: 'start launch agent', run: () => (view.newSessionOpen = true) },
    { id: 'a-new-project', group: 'Actions', label: 'New project', run: () => view.openSettings('projects') },
    {
      id: 'a-theme',
      group: 'Actions',
      label: `Switch to ${p.theme === 'glacier' ? 'Dusk' : 'Glacier'}`,
      keywords: 'theme',
      run: () => void prefs.update({ theme: p.theme === 'glacier' ? 'dusk' : 'glacier' }),
    },
    { id: 'a-zoom-in', group: 'Actions', label: 'Zoom in', hint: 'Ctrl +', keywords: 'text size larger', run: () => void prefs.zoomBy(1) },
    { id: 'a-zoom-out', group: 'Actions', label: 'Zoom out', hint: 'Ctrl −', keywords: 'text size smaller', run: () => void prefs.zoomBy(-1) },
    { id: 'a-zoom-reset', group: 'Actions', label: 'Reset zoom', hint: 'Ctrl 0', keywords: 'text size', run: () => void prefs.zoomBy(0) },
    {
      id: 'a-strip',
      group: 'Actions',
      label: p.sessionStrip ? 'Hide the session strip' : 'Show the session strip',
      keywords: 'filmstrip',
      run: () => void prefs.update({ sessionStrip: !p.sessionStrip }),
    },
    { id: 'a-group-status', group: 'Actions', label: 'Group the Overview by status', run: () => void prefs.update({ groupBy: 'status' }) },
    { id: 'a-group-project', group: 'Actions', label: 'Group the Overview by project', run: () => void prefs.update({ groupBy: 'project' }) },
    { id: 'a-group-none', group: 'Actions', label: "Don't group the Overview", run: () => void prefs.update({ groupBy: 'none' }) },
  ]
})

const all = computed(() =>
  props.mode === 'projects'
    ? projectItems.value
    : [...sessionItems.value, ...projectItems.value, ...placeItems.value, ...actionItems.value],
)
const shown = computed(() => filterItems(all.value, query.value, 60))

/** Shown items with a group heading where the group changes. */
const rows = computed(() => shown.value.map((item, i) => ({ item, heading: i === 0 || shown.value[i - 1]!.group !== item.group ? item.group : null })))

watch(query, () => (selected.value = 0))
watch(selected, async () => {
  await nextTick()
  listEl.value?.querySelector('.on')?.scrollIntoView({ block: 'nearest' })
})

function run(item: PaletteItem | undefined): void {
  if (!item) return
  close()
  item.run()
}

function onKey(e: KeyboardEvent): void {
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault()
    selected.value = moveSelection(selected.value, e.key === 'ArrowDown' ? 1 : -1, shown.value.length)
  } else if (e.key === 'Enter') {
    e.preventDefault()
    run(shown.value[selected.value])
  } else if (e.key === 'Escape') {
    e.preventDefault()
    close()
  }
}

// Focus goes back where it was (the terminal, usually) unless running an item moved it.
const before = document.activeElement instanceof HTMLElement ? document.activeElement : null
onMounted(() => input.value?.focus())
onBeforeUnmount(() => {
  if (before?.isConnected) before.focus()
})
</script>

<template>
  <div class="backdrop" @mousedown.self="close">
    <div class="palette" role="dialog" aria-modal="true" :aria-label="mode === 'projects' ? 'Switch project' : 'Command palette'">
      <label class="field">
        <Icon name="search" :size="16" />
        <input
          ref="input"
          v-model="query"
          :placeholder="mode === 'projects' ? 'Switch to a project…' : 'Search sessions and projects, or run a command…'"
          spellcheck="false"
          autocomplete="off"
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-list"
          @keydown="onKey"
        />
        <kbd>{{ mode === 'projects' ? 'Ctrl G' : 'Ctrl K' }}</kbd>
      </label>
      <ul id="palette-list" ref="listEl" class="list scroll" role="listbox">
        <template v-for="(r, i) in rows" :key="r.item.id">
          <li v-if="r.heading" class="heading" role="presentation">{{ r.heading }}</li>
          <li
            class="item"
            :class="{ on: i === selected }"
            role="option"
            :aria-selected="i === selected"
            @mousemove="selected = i"
            @click="run(r.item)"
          >
            <span class="label">{{ r.item.label }}</span>
            <span v-if="r.item.hint" class="hint">{{ r.item.hint }}</span>
          </li>
        </template>
        <li v-if="!rows.length" class="empty">Nothing matches.</li>
      </ul>
      <p class="foot"><kbd>↑</kbd><kbd>↓</kbd> choose · <kbd>Enter</kbd> open · <kbd>Esc</kbd> close</p>
    </div>
  </div>
</template>

<style scoped>
.backdrop {
  position: fixed;
  inset: 0;
  z-index: 45;
  background: rgb(0 0 0 / 0.35);
  display: flex;
  justify-content: center;
  align-items: flex-start;
  padding: 12vh 16px 16px;
}
.palette {
  width: min(620px, 100%);
  max-height: 70vh;
  display: flex;
  flex-direction: column;
  border-radius: 14px;
  background: var(--surface-raised);
  border: 1px solid var(--border-strong);
  box-shadow: 0 24px 60px rgb(0 0 0 / 0.5);
  overflow: hidden;
  animation: rise 160ms ease both;
}
@keyframes rise {
  from {
    opacity: 0;
    transform: translateY(-6px);
  }
}
.field {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 14px;
  border-bottom: 1px solid var(--border);
  color: var(--text-muted);
}
.field input {
  flex: 1;
  border: 0;
  background: transparent;
  padding: 2px 0;
  font-size: 15px;
  outline: none;
}
kbd {
  font-size: 10.5px;
  padding: 1px 6px;
  border-radius: 5px;
  background: var(--surface-chip);
  color: var(--text-2);
}
.list {
  list-style: none;
  margin: 0;
  padding: 6px;
  overflow: auto;
  flex: 1;
}
.heading {
  padding: 10px 10px 4px;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-muted);
}
.item {
  display: flex;
  align-items: baseline;
  gap: 12px;
  padding: 8px 10px;
  border-radius: 8px;
  cursor: pointer;
}
.item.on {
  background: var(--accent-soft);
}
.label {
  font-size: 14px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.hint {
  margin-left: auto;
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  min-width: 0;
}
.empty {
  padding: 14px 10px;
  color: var(--text-muted);
  font-size: 13px;
}
.foot {
  margin: 0;
  padding: 8px 14px;
  border-top: 1px solid var(--border);
  font-size: 11.5px;
  color: var(--text-muted);
  display: flex;
  gap: 5px;
  align-items: center;
}
</style>
