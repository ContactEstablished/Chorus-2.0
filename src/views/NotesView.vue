<script setup lang="ts">
/**
 * Notes (P8, D17): project-tagged or untagged context added to agents' instructions at each run
 * start. Pinned notes come first. Untagged notes apply to every project.
 */
import { computed, onMounted, ref } from 'vue'
import { commands, unwrap, type Note, type NoteSave } from '../ipc'
import { useProjectsStore } from '../stores/projects'
import { useNoticesStore } from '../stores/notices'

const projects = useProjectsStore()
const notices = useNoticesStore()

const list = ref<Note[]>([])
/** 'all', 'untagged', or a project id. */
const filter = ref<'all' | 'untagged' | number>('all')
const editing = ref<NoteSave | null>(null)
const error = ref<string | null>(null)
const busy = ref(false)

async function load(): Promise<void> {
  try {
    list.value = await unwrap(commands.notesList())
  } catch (e) {
    notices.error("Couldn't load notes.", e)
  }
}
onMounted(() => {
  void load()
  if (!projects.list.length) void projects.load()
})

const shown = computed(() =>
  list.value.filter((n) =>
    filter.value === 'all' ? true : filter.value === 'untagged' ? n.projectId === null : n.projectId === filter.value,
  ),
)
const counts = computed(() => {
  const m = new Map<number | null, number>()
  for (const n of list.value) m.set(n.projectId, (m.get(n.projectId) ?? 0) + 1)
  return m
})

function startNew(): void {
  error.value = null
  editing.value = { id: null, projectId: typeof filter.value === 'number' ? filter.value : null, title: '', body: '', pinned: false }
}
function edit(n: Note): void {
  error.value = null
  editing.value = { id: n.id, projectId: n.projectId, title: n.title, body: n.body, pinned: n.pinned }
}

async function save(): Promise<void> {
  if (!editing.value) return
  busy.value = true
  error.value = null
  try {
    await unwrap(commands.noteSave(editing.value))
    editing.value = null
    await load()
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = false
  }
}

async function remove(): Promise<void> {
  const id = editing.value?.id
  if (id === null || id === undefined) return
  try {
    await unwrap(commands.noteDelete(id))
    editing.value = null
    await load()
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
}

function projectName(id: number | null): string {
  return id === null ? 'All projects' : (projects.byId.get(id)?.name ?? '(deleted project)')
}
function projectColour(id: number | null): string | undefined {
  return id === null ? undefined : projects.byId.get(id)?.colour
}
function age(secs: number): string {
  const d = new Date(secs * 1000)
  const today = new Date()
  if (d.toDateString() === today.toDateString()) return 'today'
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}
</script>

<template>
  <section class="page">
    <header class="head">
      <div>
        <h1>Notes</h1>
        <p class="lead">Context you want agents to keep: conventions, decisions, reminders. Each session gets the notes for its project when its agent starts, and on every Restart.</p>
      </div>
      <button type="button" class="primary" @click="startNew">New note</button>
    </header>

    <div class="filters" role="group" aria-label="Project">
      <span class="label">Project</span>
      <button type="button" class="pill" :class="{ on: filter === 'all' }" @click="filter = 'all'">All <span class="n">{{ list.length }}</span></button>
      <button type="button" class="pill" :class="{ on: filter === 'untagged' }" @click="filter = 'untagged'">
        Untagged <span class="n">{{ counts.get(null) ?? 0 }}</span>
      </button>
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
      <span class="aside">Untagged notes apply to every project</span>
    </div>

    <form v-if="editing" class="editor" @submit.prevent="save">
      <div class="row">
        <input v-model="editing.title" placeholder="Title" maxlength="120" aria-label="Title" autocomplete="off" />
        <select v-model="editing.projectId" aria-label="Project">
          <option :value="null">All projects</option>
          <option v-for="p in projects.usable" :key="p.id" :value="p.id">{{ p.name }}</option>
        </select>
        <label class="pin"><input v-model="editing.pinned" type="checkbox" /> Pinned</label>
      </div>
      <textarea v-model="editing.body" rows="8" placeholder="What agents should know (Markdown)" aria-label="Note" />
      <p class="hint">Keys don't belong in notes: a note that holds one is refused.</p>
      <p v-if="error" class="hint warn">{{ error }}</p>
      <div class="actions">
        <button v-if="editing.id !== null" type="button" class="danger" @click="remove">Delete</button>
        <span class="grow" />
        <button type="button" class="quiet" @click="editing = null">Cancel</button>
        <button type="submit" class="primary" :disabled="busy || !editing.title.trim()">Save note</button>
      </div>
    </form>

    <div v-if="shown.length" class="table">
      <button v-for="n in shown" :key="n.id" type="button" class="note" @click="edit(n)">
        <span class="title">{{ n.title }}</span>
        <span class="proj" :class="{ untagged: n.projectId === null }" :style="{ '--tag': projectColour(n.projectId) }">
          <span class="sq" />{{ projectName(n.projectId) }}
        </span>
        <span class="body">{{ n.body }}</span>
        <span class="when" :class="{ pinned: n.pinned }">{{ n.pinned ? 'pinned' : age(n.updatedAtSecs) }}</span>
      </button>
    </div>
    <p v-else class="empty">
      {{ list.length ? 'No notes for this filter.' : 'No notes yet. A note tagged with a project reaches that project’s sessions; an untagged one reaches all.' }}
    </p>
  </section>
</template>

<style scoped>
.page {
  padding: 28px 40px;
  max-width: 1040px;
}
.head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 20px;
}
h1 {
  margin: 0 0 6px;
  font-size: 28px;
}
.lead {
  margin: 0 0 20px;
  color: var(--text-2);
  max-width: 640px;
  line-height: 1.45;
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
  border-radius: var(--radius-control);
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
.editor {
  display: grid;
  gap: 10px;
  padding: 16px;
  border: 1px solid var(--accent-line);
  border-radius: var(--radius-card);
  background: var(--surface-raised);
  margin-bottom: 18px;
}
.editor .row {
  display: flex;
  gap: 10px;
  align-items: center;
}
.editor .row input:first-child {
  flex: 1;
}
.pin {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  color: var(--text-2);
  white-space: nowrap;
}
textarea {
  resize: vertical;
  font-family: inherit;
  line-height: 1.45;
}
.hint {
  margin: 0;
  font-size: 12.5px;
  color: var(--text-muted);
}
.hint.warn {
  color: var(--status-failed);
}
.actions {
  display: flex;
  gap: 8px;
}
.grow {
  flex: 1;
}
.table {
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  overflow: hidden;
  background: var(--surface);
}
.note {
  display: grid;
  grid-template-columns: minmax(160px, 1.1fr) minmax(110px, 0.7fr) 2fr 70px;
  gap: 16px;
  align-items: center;
  width: 100%;
  text-align: left;
  padding: 16px 20px;
  border: 0;
  border-bottom: 1px solid var(--border);
  border-radius: 0;
  background: transparent;
}
.note:last-child {
  border-bottom: 0;
}
.note:hover {
  background: var(--surface-raised);
}
.title {
  font-weight: 700;
}
.proj {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  font-size: 12.5px;
  font-weight: 600;
}
.proj.untagged .sq {
  background: transparent;
  border: 1px solid var(--text-muted);
}
.body {
  color: var(--text-2);
  font-size: 13.5px;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  white-space: pre-line;
}
.when {
  text-align: right;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-muted);
}
.when.pinned {
  color: var(--accent);
}
.empty {
  color: var(--text-muted);
}
</style>
