<script setup lang="ts">
/**
 * Grid View (D113): every session in the chosen project (or all, with no project chosen), live or
 * not, as live Raw terminals in a grid that resizes itself to fill the window. Each terminal is the
 * real one: typing goes to the agent, and the PTY takes the tile's size until a bigger view attaches.
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import Icon from '../../components/Icon.vue'
import SessionTerminal from '../../raw/SessionTerminal.vue'
import { useSessionsStore } from '../../stores/sessions'
import { useProjectsStore } from '../../stores/projects'
import { useViewStore } from '../../stores/view'
import { useNoticesStore } from '../../stores/notices'
import { ATTENTION, byUrgency } from '../../shell/status'
import AgentLogo from '../../components/AgentLogo.vue'
import { MIN_TILE, gridShape } from './gridShape'

const sessions = useSessionsStore()
const projects = useProjectsStore()
const view = useViewStore()
const notices = useNoticesStore()

const GAP = 12

const project = computed(() => (view.projectFilter === null ? null : (projects.byId.get(view.projectFilter) ?? null)))
const shown = computed(() =>
  (view.projectFilter === null ? sessions.list : sessions.list.filter((s) => s.projectId === view.projectFilter)).slice().sort(byUrgency),
)

const tilesEl = ref<HTMLElement | null>(null)
const box = ref({ width: 0, height: 0 })
const observer = new ResizeObserver(([e]) => {
  if (e) box.value = { width: e.contentRect.width, height: e.contentRect.height }
})
onMounted(() => tilesEl.value && observer.observe(tilesEl.value))
onBeforeUnmount(() => observer.disconnect())

const shape = computed(() => gridShape(shown.value.length, box.value.width, box.value.height, GAP))
const gridStyle = computed(() => ({
  gap: `${GAP}px`,
  gridTemplateColumns: `repeat(${shape.value.cols}, minmax(0, 1fr))`,
  gridAutoRows: `${shape.value.scroll ? MIN_TILE.height : Math.max(0, Math.floor((box.value.height - GAP * (shape.value.rows - 1)) / shape.value.rows))}px`,
}))

function filmstrip(): void {
  view.openFilmstrip(view.projectFilter, shown.value[0]?.id ?? null)
}
</script>

<template>
  <section class="grid-view">
    <header class="head">
      <h1>
        <span v-if="project" class="sq" :style="{ background: project.colour }" />
        {{ project ? project.name : 'All projects' }}
      </h1>
      <span class="count">{{ shown.length }} session{{ shown.length === 1 ? '' : 's' }}</span>
      <button class="filmstrip" title="One session at a time, with the project's others in a strip above" @click="filmstrip">
        <Icon name="sessions" :size="15" /> Filmstrip View
      </button>
    </header>

    <p v-if="!shown.length" class="none">
      No sessions {{ project ? `in ${project.name}` : 'yet' }}.
      <button class="quiet link" @click="view.newSessionOpen = true">Start one</button>
    </p>

    <div ref="tilesEl" class="tiles" :class="{ scroll: shape.scroll }" :style="gridStyle">
      <article v-for="s in shown" :key="s.id" class="tile" :class="`st-${s.attention}`">
        <header class="tile-head">
          <span class="dot" :class="{ pulse: s.attention === 'working' && !s.noSignal }" />
          <AgentLogo :agent="s.agent" />
          <span class="tname" :title="s.cwd">{{ s.name }}</span>
          <span v-if="!project && s.projectId !== null && projects.byId.get(s.projectId)" class="project-tag" :style="{ '--tag': projects.byId.get(s.projectId)!.colour }">{{
            projects.byId.get(s.projectId)!.name
          }}</span>
          <span class="tstate">{{ ATTENTION[s.attention].label }}</span>
          <button class="quiet open" title="Open the session" @click="view.openSession(s.id)">Open</button>
        </header>
        <div class="term">
          <SessionTerminal :session-id="s.id" :live="s.live" :autofocus="false" @error="(m) => notices.error(m)" />
        </div>
      </article>
    </div>
  </section>
</template>

<style scoped>
.grid-view {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 20px 24px 16px;
}
.head {
  display: flex;
  align-items: center;
  gap: 14px;
  margin-bottom: 14px;
  flex: none;
}
h1 {
  margin: 0;
  font-size: 24px;
  display: flex;
  align-items: center;
  gap: 10px;
}
.sq {
  width: 12px;
  height: 12px;
  border-radius: 3px;
}
.count {
  color: var(--text-muted);
  font-size: 13px;
}
.filmstrip {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  font-size: 13px;
}
.none {
  color: var(--text-muted);
}
.link {
  padding: 0;
  color: var(--accent);
}
.tiles {
  flex: 1;
  min-height: 0;
  display: grid;
  overflow: hidden;
}
.tiles.scroll {
  overflow: auto;
}
.tile {
  display: flex;
  flex-direction: column;
  min-height: 0;
  min-width: 0;
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
</style>
