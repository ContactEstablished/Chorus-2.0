<script setup lang="ts">
/**
 * The Overview (glacier-overview.png): a headline, status filters, project chips, and session cards
 * grouped by status (or project, or not at all).
 */
import { computed, nextTick, onMounted, onUnmounted, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useInboxStore } from '../stores/inbox'
import { inboxKey } from '../shell/inbox'
import Icon from '../components/Icon.vue'
import SessionCard from './SessionCard.vue'
import type { GroupBy } from '../ipc'
import { useSessionsStore } from '../stores/sessions'
import { useProjectsStore } from '../stores/projects'
import { usePrefsStore } from '../stores/prefs'
import { useViewStore } from '../stores/view'
import { useAppStore } from '../stores/app'
import { counts, groupSessions, headline, matchesFilter, needsYou, type StatusFilter } from '../shell/status'
import { useNow } from '../shell/now'

const sessions = useSessionsStore()
const projects = useProjectsStore()
const prefs = usePrefsStore()
const view = useViewStore()
const app = useAppStore()
const now = useNow(1000)
const inbox = useInboxStore()

const { filter } = storeToRefs(inbox)

/** Sessions in the chosen project (or all). */
const inProject = computed(() =>
  view.projectFilter === null ? sessions.list : sessions.list.filter((s) => s.projectId === view.projectFilter),
)
const c = computed(() => counts(inProject.value))
const shown = computed(() => inProject.value.filter((s) => matchesFilter(s, filter.value)))
const groups = computed(() => groupSessions(shown.value, prefs.prefs.groupBy, projects.list))
const waitingIds = computed(() => groups.value.flatMap((g) => g.sessions).filter((s) => needsYou(s.attention)).map((s) => s.id))

function openCard(id: number): void {
  if (waitingIds.value.includes(id)) inbox.open(id, waitingIds.value)
  else inbox.returning = null
  view.openSession(id)
}

async function focusSelected(): Promise<void> {
  await nextTick()
  const card = document.querySelector<HTMLElement>(`[data-inbox-id="${inbox.selectedId}"]`)
  card?.focus({ preventScroll: true })
  card?.scrollIntoView({ block: 'nearest' })
}

function onInboxKey(e: KeyboardEvent): void {
  if (view.palette || view.newSessionOpen || document.querySelector('[aria-modal="true"], [role="alertdialog"], dialog[open]')) return
  const target = e.target instanceof Element ? e.target : null
  const protectedTarget = !!target?.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"]), [role="textbox"], .xterm')
  const action = inboxKey(e, protectedTarget)
  // A button or a card keeps its own Enter handler; only the empty board opens the selection.
  if (action === 'open' && target?.closest('button, a, [role="button"]')) return
  if (action === 'next' || action === 'previous') {
    if (!waitingIds.value.length) return
    inbox.move(waitingIds.value, action === 'next' ? 1 : -1)
    void focusSelected()
  } else if (action === 'open' && inbox.selectedId !== null && waitingIds.value.includes(inbox.selectedId)) {
    openCard(inbox.selectedId)
  } else return
  e.preventDefault()
  e.stopPropagation()
}

watch(waitingIds, (ids) => {
  if (inbox.selectedId !== null && !ids.includes(inbox.selectedId)) inbox.selectedId = ids[0] ?? null
})
onMounted(() => {
  inbox.returned(waitingIds.value)
  window.addEventListener('keydown', onInboxKey)
})
onUnmounted(() => window.removeEventListener('keydown', onInboxKey))

const chosenProject = computed(() => (view.projectFilter === null ? null : (projects.byId.get(view.projectFilter) ?? null)))

const PILLS: { key: StatusFilter; label: string; st: string | null }[] = [
  { key: 'all', label: 'All', st: null },
  { key: 'needs', label: 'Needs you', st: 'input' },
  { key: 'working', label: 'Working', st: 'working' },
  { key: 'ready', label: 'Ready', st: 'ready' },
  { key: 'failed', label: 'Failed', st: 'failed' },
]
const pillCount = (k: StatusFilter) => (k === 'all' ? c.value.total : c.value[k])

/** Per project chip: its sessions, and whether one needs you. */
const chips = computed(() =>
  projects.usable.map((p) => {
    const mine = sessions.list.filter((s) => s.projectId === p.id)
    return { project: p, count: mine.length, attention: mine.some((s) => needsYou(s.attention)) }
  }),
)

const GROUPS: { key: GroupBy; label: string }[] = [
  { key: 'status', label: 'Status' },
  { key: 'project', label: 'Project' },
  { key: 'none', label: 'None' },
]

function groupTone(key: string): string {
  return key === 'needs' ? 'tone-needs' : key === 'flight' ? 'tone-flight' : ''
}
</script>

<template>
  <section class="overview">
    <header class="head">
      <div>
        <h1>{{ chosenProject ? chosenProject.name : 'Overview' }}</h1>
        <p class="headline">{{ headline(c) }}</p>
      </div>
      <div class="pills" role="radiogroup" aria-label="Show sessions">
        <button
          v-for="p in PILLS"
          :key="p.key"
          class="pill"
          :class="[{ on: filter === p.key }, p.st ? `st-${p.st}` : '']"
          role="radio"
          :aria-checked="filter === p.key"
          @click="filter = p.key"
        >
          <span class="dot" :class="{ neutral: !p.st }" />{{ p.label }} <span class="n">{{ pillCount(p.key) }}</span>
        </button>
      </div>
    </header>

    <div class="toolbar">
      <span class="tl">Project</span>
      <div class="chips">
        <button class="chip" :class="{ on: view.projectFilter === null }" @click="view.projectFilter = null">
          <span class="sq neutral" />All <span class="n">{{ sessions.list.length }}</span>
        </button>
        <button
          v-for="x in chips"
          :key="x.project.id"
          class="chip"
          :class="{ on: view.projectFilter === x.project.id, hidden: x.project.state === 'hidden' }"
          :title="x.project.state === 'hidden' ? 'Hidden from the rail' : (x.project.folder ?? '')"
          @click="view.projectFilter = x.project.id"
        >
          <span class="sq" :style="{ background: x.project.colour }" />{{ x.project.name }} <span class="n">{{ x.count }}</span>
          <span v-if="x.attention" class="attn" />
        </button>
        <button class="chip add" @click="view.openSettings('projects')"><Icon name="plus" :size="12" /> New project</button>
      </div>
      <span class="tl right">Group by</span>
      <div class="segmented" role="radiogroup" aria-label="Group by">
        <button
          v-for="g in GROUPS"
          :key="g.key"
          role="radio"
          :aria-checked="prefs.prefs.groupBy === g.key"
          :class="{ on: prefs.prefs.groupBy === g.key }"
          @click="prefs.update({ groupBy: g.key })"
        >
          {{ g.label }}
        </button>
      </div>
    </div>

    <div v-if="!sessions.list.length" class="empty">
      <p class="title">No sessions yet</p>
      <p>Start an agent or a shell in a folder. It keeps running when you switch views, and comes back when Chorus restarts.</p>
      <button class="primary" @click="view.newSessionOpen = true"><Icon name="plus" :size="14" /> New session</button>
    </div>
    <p v-else-if="!shown.length" class="none">No sessions match. <button class="quiet link" @click="filter = 'all'; view.projectFilter = null">Show all</button></p>

    <p v-if="waitingIds.length" class="inbox-help">j / k select waiting session · Enter open · Esc back</p>

    <section v-for="g in groups" :key="g.key" class="group">
      <h2 class="gtitle" :class="groupTone(g.key)">
        <span v-if="g.colour" class="sq" :style="{ background: g.colour }" />
        <span class="t">{{ g.title }}</span>
        <span class="hint">{{ g.hint }}</span>
      </h2>
      <div class="cards">
        <SessionCard
          v-for="s in g.sessions"
          :key="s.id"
          :session="s"
          :data-inbox-id="needsYou(s.attention) ? s.id : undefined"
          :class="{ 'inbox-selected': inbox.selectedId === s.id }"
          :aria-current="inbox.selectedId === s.id ? 'true' : undefined"
          @focus="inbox.selectedId = needsYou(s.attention) ? s.id : null"
          :project="s.projectId === null ? null : (projects.byId.get(s.projectId) ?? null)"
          :home="app.info?.homeDir ?? null"
          :now="now"
          @open="openCard(s.id)"
        />
      </div>
    </section>
  </section>
</template>

<style scoped>
.inbox-help { color: var(--text-muted); font-size: 12px; margin: 0 0 16px; }
.overview {
  padding: 28px 40px 48px;
}
.head {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 20px;
  flex-wrap: wrap;
  margin-bottom: 22px;
}
h1 {
  margin: 0;
  font-size: 30px;
}
.headline {
  margin: 8px 0 0;
  color: var(--text-2);
  font-size: 15px;
}
.pills {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.pill {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 6px 12px;
  border-radius: var(--radius-pill);
  background: transparent;
  border: 1px solid var(--border-strong);
  color: var(--text);
  font-weight: 500;
  font-size: 13px;
}
.pill.on {
  background: var(--surface-chip);
  border-color: var(--border-strong);
  font-weight: 600;
}
.dot.neutral {
  background: var(--text-muted);
}
.n {
  color: var(--text-muted);
  font-weight: 500;
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  padding: 10px 14px;
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  margin-bottom: 26px;
}
.tl {
  color: var(--text-muted);
  font-size: 13px;
  font-weight: 500;
}
.tl.right {
  margin-left: auto;
}
.chips {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.chip {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 5px 11px;
  border-radius: 8px;
  background: transparent;
  border: 1px solid var(--border-strong);
  font-size: 13px;
  font-weight: 500;
}
.chip.on {
  background: var(--accent-soft);
  border-color: var(--accent-line);
  font-weight: 600;
}
.chip.hidden {
  border-style: dashed;
}
.chip.add {
  border-style: dashed;
  color: var(--text-muted);
  gap: 4px;
}
.sq {
  width: 8px;
  height: 8px;
  border-radius: 2px;
  flex: none;
}
.sq.neutral {
  background: var(--text-muted);
}
.attn {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--status-needs);
}
.segmented {
  display: inline-flex;
  padding: 3px;
  gap: 2px;
  border-radius: 10px;
  background: var(--bg);
  border: 1px solid var(--border);
}
.segmented button {
  border: 0;
  background: transparent;
  padding: 5px 13px;
  color: var(--text-2);
  font-weight: 500;
  font-size: 13px;
}
.segmented button.on {
  background: var(--surface-chip);
  color: var(--text);
  font-weight: 600;
}
.group {
  margin-bottom: 30px;
}
.gtitle {
  display: flex;
  align-items: baseline;
  gap: 10px;
  margin: 0 0 12px;
  font-family: var(--font-ui);
  font-size: 14px;
  letter-spacing: 0.04em;
}
.gtitle .t {
  text-transform: uppercase;
  font-weight: 700;
}
.gtitle .sq {
  align-self: center;
}
.gtitle.tone-needs .t {
  color: var(--status-needs);
}
.gtitle.tone-flight .t {
  color: var(--status-working);
}
.gtitle .hint {
  color: var(--text-muted);
  font-weight: 400;
  font-size: 13px;
  letter-spacing: 0;
}
.cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(min(320px, 100%), 1fr));
  gap: 16px;
}
.empty {
  max-width: 520px;
  padding: 28px;
  border: 1px dashed var(--border-strong);
  border-radius: var(--radius-card);
  color: var(--text-2);
}
.empty .title {
  margin: 0 0 6px;
  color: var(--text);
  font-weight: 600;
  font-size: 16px;
}
.empty .primary {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin-top: 8px;
}
.none {
  color: var(--text-muted);
}
.link {
  padding: 0;
  color: var(--accent);
}
@media (max-width: 860px) {
  .overview {
    padding: 22px 20px 40px;
  }
  .tl.right {
    margin-left: 0;
  }
}
</style>
