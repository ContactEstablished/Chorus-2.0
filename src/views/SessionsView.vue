<script setup lang="ts">
/**
 * Sessions: every open session, searchable, as a list or as the Wall of live Raw terminals (D26).
 * On the Wall each terminal is the real one: typing goes to the agent, and the PTY takes the tile's
 * size until a bigger view attaches (as Chorus 1's grid did). In the list, Select picks sessions,
 * or all the search shows, to close together (D104).
 */
import { computed, ref, watch } from 'vue'
import Icon from '../components/Icon.vue'
import SessionTerminal from '../raw/SessionTerminal.vue'
import CloseSessionsDialog from './CloseSessionsDialog.vue'
import type { SessionSummary } from '../ipc'
import { useSessionsStore } from '../stores/sessions'
import { useProjectsStore } from '../stores/projects'
import { useViewStore } from '../stores/view'
import { useAppStore } from '../stores/app'
import { useNoticesStore } from '../stores/notices'
import { AGENT_LABELS } from '../raw/labels'
import { ATTENTION, ago, byUrgency, lastActive, shortPath } from '../shell/status'
import AgentLogo from '../components/AgentLogo.vue'
import { pickable, pickedState } from '../shell/bulkClose'
import { useNow } from '../shell/now'

const sessions = useSessionsStore()
const projects = useProjectsStore()
const view = useViewStore()
const app = useAppStore()
const notices = useNoticesStore()
const now = useNow()

const query = ref('')
const order = ref<'urgency' | 'recent' | 'name'>('urgency')

function haystack(s: SessionSummary): string {
  const project = s.projectId === null ? '' : (projects.byId.get(s.projectId)?.name ?? '')
  return [s.name, s.description, s.cwd, AGENT_LABELS[s.agent], s.providerName, project, ATTENTION[s.attention].label, s.latest, ...s.tags, s.worktree?.branch]
    .filter(Boolean)
    .join('\n')
    .toLowerCase()
}

const shown = computed(() => {
  const words = query.value.toLowerCase().split(/\s+/).filter(Boolean)
  const list = sessions.list.filter((s) => {
    const h = haystack(s)
    return words.every((w) => h.includes(w))
  })
  const sort = {
    urgency: byUrgency,
    recent: (a: SessionSummary, b: SessionSummary) => lastActive(b) - lastActive(a),
    name: (a: SessionSummary, b: SessionSummary) => a.name.localeCompare(b.name),
  }[order.value]
  return [...list].sort(sort)
})

const project = (s: SessionSummary) => (s.projectId === null ? null : (projects.byId.get(s.projectId) ?? null))

// Select: pick sessions in the list and close them together.
const selecting = ref(false)
const picked = ref(new Set<number>())
/** The sessions the confirm dialog closes, fixed when it opens. */
const confirming = ref<SessionSummary[] | null>(null)
const pickedShown = computed(() => shown.value.filter((s) => picked.value.has(s.id)))
const allState = computed(() => pickedState(shown.value, picked.value))
const canPick = computed(() => shown.value.filter(pickable).length)
const lockedShown = computed(() => shown.value.length - canPick.value)

// What's picked is always on screen and closable: a session the search hides, or that closes or
// is locked, drops out.
watch(shown, (list) => {
  const ids = new Set(list.filter(pickable).map((s) => s.id))
  if ([...picked.value].some((id) => !ids.has(id))) picked.value = new Set([...picked.value].filter((id) => ids.has(id)))
})
watch(
  () => view.sessionsLayout,
  () => stopSelecting(),
)

function stopSelecting(): void {
  selecting.value = false
  picked.value = new Set()
}
function togglePick(s: SessionSummary): void {
  if (!pickable(s)) return
  const next = new Set(picked.value)
  if (!next.delete(s.id)) next.add(s.id)
  picked.value = next
}
function toggleAll(): void {
  picked.value = allState.value === 'all' ? new Set() : new Set(shown.value.filter(pickable).map((s) => s.id))
}
function onDialogClose(ran: boolean): void {
  confirming.value = null
  if (ran) stopSelecting()
}
</script>

<template>
  <section class="sessions" :class="{ wall: view.sessionsLayout === 'wall' }">
    <header class="head">
      <h1>Sessions</h1>
      <div class="tools">
        <label class="search">
          <Icon name="search" :size="15" />
          <input v-model="query" type="search" placeholder="Search name, folder, project, tag, agent…" aria-label="Search sessions" spellcheck="false" />
        </label>
        <select v-if="view.sessionsLayout === 'list'" v-model="order" aria-label="Order">
          <option value="urgency">Most urgent</option>
          <option value="recent">Most recent</option>
          <option value="name">Name</option>
        </select>
        <button
          v-if="view.sessionsLayout === 'list' && sessions.list.length"
          class="select-toggle"
          :class="{ on: selecting }"
          :aria-pressed="selecting"
          title="Pick sessions, or all the search shows, to close together"
          @click="selecting ? stopSelecting() : (selecting = true)"
        >
          <Icon name="check" :size="15" /> Select
        </button>
        <div class="segmented" role="radiogroup" aria-label="Layout">
          <button role="radio" :aria-checked="view.sessionsLayout === 'list'" :class="{ on: view.sessionsLayout === 'list' }" title="List" @click="view.sessionsLayout = 'list'">
            <Icon name="sessions" :size="15" /> List
          </button>
          <button role="radio" :aria-checked="view.sessionsLayout === 'wall'" :class="{ on: view.sessionsLayout === 'wall' }" title="Wall of live terminals" @click="view.sessionsLayout = 'wall'">
            <Icon name="wall" :size="15" /> Wall
          </button>
        </div>
      </div>
    </header>

    <p v-if="!sessions.list.length" class="none">
      No sessions yet. <button class="quiet link" @click="view.newSessionOpen = true">Start one</button>
    </p>
    <p v-else-if="!shown.length" class="none">Nothing matches “{{ query }}”.</p>

    <div v-if="selecting && view.sessionsLayout === 'list' && sessions.list.length" class="pickbar" role="toolbar" aria-label="Selected sessions">
      <label class="all">
        <input type="checkbox" :checked="allState === 'all'" :indeterminate="allState === 'some'" :disabled="!canPick" @change="toggleAll" />
        Select all {{ canPick }}{{ query.trim() ? ' matching' : '' }}
      </label>
      <span class="count">{{ pickedShown.length }} selected</span>
      <span v-if="lockedShown" class="muted">{{ lockedShown }} locked: unlock {{ lockedShown === 1 ? 'it' : 'them' }} to close</span>
      <span class="spacer" />
      <button class="danger" :disabled="!pickedShown.length" @click="confirming = [...pickedShown]">
        <Icon name="trash" :size="14" /> Close {{ pickedShown.length || '' }}…
      </button>
      <button class="quiet" @click="stopSelecting">Cancel</button>
    </div>

    <ul v-if="view.sessionsLayout === 'list'" class="list">
      <li v-for="s in shown" :key="s.id">
        <button
          class="row"
          :class="[`st-${s.attention}`, { picking: selecting, picked: selecting && picked.has(s.id) }]"
          :role="selecting ? 'checkbox' : undefined"
          :aria-checked="selecting ? picked.has(s.id) : undefined"
          :disabled="selecting && !pickable(s)"
          :title="selecting && !pickable(s) ? 'Locked: unlock it to close it' : undefined"
          @click="selecting ? togglePick(s) : view.openSession(s.id)"
        >
          <span v-if="selecting" class="box" aria-hidden="true"><Icon v-if="picked.has(s.id)" name="check" :size="12" /></span>
          <span class="dot" :class="{ pulse: s.attention === 'working' && !s.noSignal }" />
          <AgentLogo :agent="s.agent" />
          <span class="main">
            <span class="name">{{ s.name }}<span v-if="s.locked" class="locked">locked</span></span>
            <span class="sub">{{ s.description ?? shortPath(s.cwd, app.info?.homeDir ?? null) }}</span>
          </span>
          <span class="proj">
            <span v-if="project(s)" class="project-tag" :style="{ '--tag': project(s)!.colour }">{{ project(s)!.name }}</span>
          </span>
          <span class="state">{{ ATTENTION[s.attention].label }}</span>
          <span class="when">{{ ago(s.latestAtSecs || s.runStartedAtSecs || s.createdAtSecs, now) }}</span>
          <Icon name="chevronRight" :size="16" class="go" />
        </button>
      </li>
    </ul>

    <div v-else class="tiles">
      <article v-for="s in shown" :key="s.id" class="tile" :class="`st-${s.attention}`">
        <header class="tile-head">
          <span class="dot" :class="{ pulse: s.attention === 'working' && !s.noSignal }" />
          <AgentLogo :agent="s.agent" />
          <span class="tname" :title="s.cwd">{{ s.name }}</span>
          <span v-if="project(s)" class="project-tag" :style="{ '--tag': project(s)!.colour }">{{ project(s)!.name }}</span>
          <span class="tstate">{{ ATTENTION[s.attention].label }}</span>
          <button class="quiet open" title="Open the session" @click="view.openSession(s.id)">Open</button>
        </header>
        <div class="term">
          <SessionTerminal :session-id="s.id" :live="s.live" :autofocus="false" @error="(m) => notices.error(m)" />
        </div>
      </article>
    </div>

    <CloseSessionsDialog v-if="confirming" :targets="confirming" @close="onDialogClose" />
  </section>
</template>

<style scoped>
.sessions {
  padding: 28px 40px 40px;
}
.sessions.wall {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding-bottom: 16px;
}
.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap;
  margin-bottom: 20px;
}
h1 {
  margin: 0;
  font-size: 28px;
}
.tools {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
}
.search {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-control);
  background: var(--surface);
  color: var(--text-muted);
  width: min(340px, 60vw);
}
.search input {
  border: 0;
  background: transparent;
  padding: 7px 0;
  flex: 1;
  outline: none;
}
.segmented {
  display: inline-flex;
  padding: 3px;
  gap: 2px;
  border-radius: 10px;
  background: var(--surface);
  border: 1px solid var(--border);
}
.segmented button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 0;
  background: transparent;
  padding: 5px 12px;
  color: var(--text-2);
  font-size: 13px;
}
.segmented button.on {
  background: var(--surface-chip);
  color: var(--text);
  font-weight: 600;
}
.select-toggle {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  font-size: 13px;
}
.select-toggle.on {
  color: var(--accent);
  background: var(--accent-soft);
  border-color: var(--accent-line);
}
.pickbar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 14px;
  margin-bottom: 10px;
  padding: 7px 10px 7px 16px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-control);
  background: var(--surface-raised);
  font-size: 13px;
}
.all {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  font-weight: 600;
  cursor: pointer;
}
.all input {
  width: 16px;
  height: 16px;
  margin: 0;
  padding: 0;
  accent-color: var(--accent);
}
.count {
  color: var(--text-2);
}
.muted {
  color: var(--text-muted);
}
.spacer {
  flex: 1;
}
.pickbar button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  font-size: 13px;
}
.none {
  color: var(--text-muted);
}
.link {
  padding: 0;
  color: var(--accent);
}
.list {
  list-style: none;
  margin: 0;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  overflow: hidden;
}
.list li + li {
  border-top: 1px solid var(--border);
}
.row {
  width: 100%;
  display: grid;
  grid-template-columns: 8px auto minmax(0, 1fr) minmax(0, 160px) 150px 80px 16px;
  align-items: center;
  gap: 14px;
  padding: 12px 16px;
  border: 0;
  border-radius: 0;
  background: var(--surface);
  text-align: left;
}
.row:hover:not(:disabled) {
  background: var(--surface-raised);
}
.row.picking {
  grid-template-columns: 16px 8px auto minmax(0, 1fr) minmax(0, 160px) 150px 80px 16px;
}
.row.picking .go {
  visibility: hidden;
}
.row.picked,
.row.picked:hover:not(:disabled) {
  background: var(--accent-soft);
}
.box {
  width: 16px;
  height: 16px;
  display: grid;
  place-items: center;
  border: 1.5px solid var(--border-strong);
  border-radius: 4px;
}
.row.picked .box {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--on-accent);
}
.main {
  display: flex;
  flex-direction: column;
  min-width: 0;
}
.name {
  font-weight: 600;
  display: flex;
  gap: 8px;
  align-items: center;
}
.locked {
  font-size: 10.5px;
  padding: 1px 7px;
  border-radius: var(--radius-pill);
  background: var(--surface-chip);
  color: var(--text-2);
  font-weight: 600;
}
.sub {
  color: var(--text-muted);
  font-size: 12.5px;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.proj {
  min-width: 0;
  overflow: hidden;
}
.state {
  color: var(--st);
  font-size: 13px;
  white-space: nowrap;
}
.row.st-done .state,
.row.st-running .state {
  color: var(--text-muted);
}
.when {
  color: var(--text-muted);
  font-size: 12.5px;
  text-align: right;
  white-space: nowrap;
}
.go {
  color: var(--text-muted);
}
.tiles {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(380px, 100%), 1fr));
  grid-auto-rows: minmax(280px, 1fr);
  gap: 12px;
  overflow: auto;
}
.tile {
  display: flex;
  flex-direction: column;
  min-height: 0;
  border: 1px solid var(--border);
  border-top: 2px solid var(--st);
  border-radius: 12px;
  overflow: hidden;
  background: var(--bg);
}
.tile.st-done,
.tile.st-running {
  border-top-color: var(--border-strong);
}
.tile:focus-within {
  border-color: var(--accent-line);
}
.tile-head {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 7px 8px 7px 12px;
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  font-size: 13px;
  min-width: 0;
}
.tname {
  font-weight: 600;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  min-width: 0;
  flex: 1;
}
.tstate {
  color: var(--st);
  white-space: nowrap;
  font-size: 12px;
}
.tile.st-done .tstate,
.tile.st-running .tstate {
  color: var(--text-muted);
}
.open {
  padding: 3px 9px;
  font-size: 12.5px;
}
.term {
  flex: 1;
  min-height: 0;
}
@media (max-width: 860px) {
  .sessions {
    padding: 22px 20px;
  }
  .row {
    grid-template-columns: 8px auto minmax(0, 1fr) 16px;
  }
  .row.picking {
    grid-template-columns: 16px 8px auto minmax(0, 1fr) 16px;
  }
  .proj,
  .state,
  .when {
    display: none;
  }
}
</style>
