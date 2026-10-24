<script setup lang="ts">
/**
 * Documents (P8): plans, specs and reports sessions wrote, each linked to its session and tagged
 * with that session's project. Open hands the file to the application Windows associates with it,
 * from Rust, after checking it is still inside the session's folder or project. A file that has
 * gone shows "missing".
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { commands, unwrap, type ChorusDocument, type DocKind } from '../ipc'
import { useProjectsStore } from '../stores/projects'
import { useViewStore } from '../stores/view'
import { useNoticesStore } from '../stores/notices'

const projects = useProjectsStore()
const view = useViewStore()
const notices = useNoticesStore()

const list = ref<ChorusDocument[]>([])
const filter = ref<'all' | number>('all')
let timer: ReturnType<typeof setInterval> | null = null

const KIND: Record<DocKind, string> = { plan: 'Plan', spec: 'Spec', report: 'Report', review: 'Review report', decision: 'Decision record' }

async function load(): Promise<void> {
  try {
    list.value = await unwrap(commands.documentsList())
  } catch (e) {
    notices.error("Couldn't load documents.", e)
  }
}
onMounted(() => {
  void load()
  if (!projects.list.length) void projects.load()
  // New documents are found within seconds of the session writing them.
  timer = setInterval(() => void load(), 10_000)
})
onUnmounted(() => {
  if (timer) clearInterval(timer)
})

const shown = computed(() => list.value.filter((d) => filter.value === 'all' || d.projectId === filter.value))
const counts = computed(() => {
  const m = new Map<number | null, number>()
  for (const d of list.value) m.set(d.projectId, (m.get(d.projectId) ?? 0) + 1)
  return m
})

async function open(d: ChorusDocument): Promise<void> {
  try {
    await unwrap(commands.documentOpen(d.id))
  } catch (e) {
    notices.error(`Couldn't open ${d.fileName}.`, e)
    void load()
  }
}

function edited(d: ChorusDocument): string {
  if (d.missing) return 'missing'
  const secs = d.editedAtSecs ?? d.firstSeenAtSecs
  const t = new Date(secs * 1000)
  if (t.toDateString() === new Date().toDateString()) return `edited ${t.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}`
  return t.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}
</script>

<template>
  <section class="page">
    <h1>Documents</h1>
    <p class="lead">Plans, specs and reports your sessions wrote, linked to the session that produced each one.</p>

    <div class="filters" role="group" aria-label="Project">
      <span class="label">Project</span>
      <button type="button" class="pill" :class="{ on: filter === 'all' }" @click="filter = 'all'">All <span class="n">{{ list.length }}</span></button>
      <button
        v-for="p in projects.usable"
        :key="p.id"
        type="button"
        class="pill"
        :class="{ on: filter === p.id }"
        :style="{ '--tag': p.colour }"
        @click="filter = p.id"
      >
        <span class="sq" />{{ p.name }} <span class="n">{{ counts.get(p.id) ?? 0 }}</span>
      </button>
      <span class="aside">Documents inherit the project of the session that created them</span>
    </div>

    <div v-if="shown.length" class="table">
      <div v-for="d in shown" :key="d.id" class="doc" :class="{ missing: d.missing }">
        <button type="button" class="name" :disabled="d.missing" :title="d.missing ? 'The file was moved or deleted' : `Open ${d.fileName}`" @click="open(d)">
          {{ d.title ?? d.fileName }}
        </button>
        <span class="proj" :style="{ '--tag': d.projectId === null ? undefined : projects.byId.get(d.projectId)?.colour }">
          <span class="sq" />{{ d.projectId === null ? 'No project' : (projects.byId.get(d.projectId)?.name ?? '(deleted project)') }}
        </span>
        <span class="kind">
          {{ KIND[d.kind] }} ·
          <button v-if="d.sessionId !== null" type="button" class="link" @click="view.openSession(d.sessionId)">{{ d.sessionName ?? 'its session' }}</button>
          <span v-else>session gone</span>
          <span class="file">{{ d.fileName }}</span>
        </span>
        <span class="when">{{ edited(d) }}</span>
      </div>
    </div>
    <p v-else class="empty">
      No documents yet. When a session writes a Markdown plan, spec, review or report at the top of its folder or under _docs/ or docs/,
      it appears here. Claude Code sessions are covered; other agents' file writes aren't reported to Chorus yet.
    </p>
  </section>
</template>

<style scoped>
.page {
  padding: 28px 40px;
  max-width: 1040px;
}
h1 {
  margin: 0 0 6px;
  font-size: 28px;
}
.lead {
  margin: 0 0 20px;
  color: var(--text-2);
}
.filters {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  padding: 12px 14px;
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  background: var(--surface);
  margin-bottom: 18px;
}
.label,
.aside {
  color: var(--text-muted);
  font-size: 12.5px;
}
.aside {
  margin-left: auto;
}
.pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  font-size: 13px;
}
.pill.on {
  border-color: var(--accent);
  background: var(--accent-soft);
}
.pill .n {
  color: var(--text-muted);
}
.sq {
  width: 7px;
  height: 7px;
  border-radius: 2px;
  background: var(--tag, var(--text-muted));
}
.table {
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  overflow: hidden;
  background: var(--surface);
}
.doc {
  display: grid;
  grid-template-columns: minmax(180px, 1.2fr) minmax(110px, 0.6fr) 2fr 120px;
  gap: 16px;
  align-items: center;
  padding: 14px 20px;
  border-bottom: 1px solid var(--border);
}
.doc:last-child {
  border-bottom: 0;
}
.name {
  justify-self: start;
  text-align: left;
  font-weight: 700;
  background: transparent;
  border: 0;
  padding: 0;
}
.name:hover:not(:disabled) {
  color: var(--accent);
}
.missing .name {
  text-decoration: line-through;
  color: var(--text-muted);
}
.proj {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  font-size: 12.5px;
  font-weight: 600;
}
.kind {
  color: var(--text-2);
  font-size: 13.5px;
}
.link {
  background: transparent;
  border: 0;
  padding: 0;
  color: var(--accent);
}
.file {
  display: block;
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-muted);
}
.when {
  text-align: right;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-muted);
}
.missing .when {
  color: var(--status-failed);
}
.empty {
  color: var(--text-muted);
  max-width: 640px;
  line-height: 1.5;
}
</style>
