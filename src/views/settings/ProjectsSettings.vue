<script setup lang="ts">
/**
 * Projects (PLAN P4): add, rename, recolour, reorder by dragging, hide, archive and delete. Hidden
 * projects leave the rail; archived ones also leave the Overview's chips and can't take new sessions.
 */
import { onMounted, ref } from 'vue'
import Icon from '../../components/Icon.vue'
import { commands, type Project, type ProjectState } from '../../ipc'
import { useProjectsStore } from '../../stores/projects'
import { useSessionsStore } from '../../stores/sessions'
import { useNoticesStore } from '../../stores/notices'

const projects = useProjectsStore()
const sessions = useSessionsStore()
const notices = useNoticesStore()

const palette = ref<string[]>([])
onMounted(async () => {
  palette.value = await commands.projectPalette().catch(() => [])
})

// Adding.
const adding = ref(false)
const newName = ref('')
const newFolder = ref('')
async function add(): Promise<void> {
  if (await projects.save(null, { name: newName.value, folder: newFolder.value.trim() || null, colour: null })) {
    notices.success(`Added ${newName.value.trim()}.`)
    newName.value = ''
    newFolder.value = ''
    adding.value = false
  }
}

// Editing one at a time.
const editing = ref<number | null>(null)
const draftName = ref('')
const draftFolder = ref('')
function edit(p: Project): void {
  editing.value = p.id
  draftName.value = p.name
  draftFolder.value = p.folder ?? ''
}
async function saveEdit(p: Project): Promise<void> {
  if (await projects.save(p.id, { name: draftName.value, folder: draftFolder.value.trim() || null, colour: p.colour })) editing.value = null
}

const colouring = ref<number | null>(null)
async function recolour(p: Project, colour: string): Promise<void> {
  colouring.value = null
  await projects.save(p.id, { name: p.name, folder: p.folder, colour })
}

const confirmingDelete = ref<number | null>(null)
async function remove(p: Project): Promise<void> {
  confirmingDelete.value = null
  if (await projects.remove(p.id)) notices.info(`Deleted ${p.name}. Its sessions keep running, in no project.`)
}

const setState = (p: Project, s: ProjectState) => projects.setState(p.id, s)

function sessionCount(id: number): number {
  return sessions.list.filter((s) => s.projectId === id).length
}

// Reordering by drag: the dragged row moves to where it's dropped.
const dragging = ref<number | null>(null)
const over = ref<number | null>(null)
async function drop(target: number): Promise<void> {
  const from = dragging.value
  dragging.value = null
  over.value = null
  if (from === null || from === target) return
  const ids = projects.list.map((p) => p.id).filter((id) => id !== from)
  ids.splice(ids.indexOf(target), 0, from)
  await projects.reorder(ids)
}
async function nudge(p: Project, by: number): Promise<void> {
  const ids = projects.list.map((x) => x.id)
  const i = ids.indexOf(p.id)
  const j = i + by
  if (j < 0 || j >= ids.length) return
  ;[ids[i], ids[j]] = [ids[j]!, ids[i]!]
  await projects.reorder(ids)
}

const STATE_LABELS: Record<ProjectState, string> = { active: 'In the rail', hidden: 'Hidden', archived: 'Archived' }
</script>

<template>
  <div>
    <div class="head">
      <div>
        <h2>Projects</h2>
        <p class="hint">
          A project groups sessions by folder: a new session whose folder is inside a project's folder joins it. Drag to
          reorder; the rail shows them in this order.
        </p>
      </div>
      <button class="primary" :disabled="adding" @click="adding = true"><Icon name="plus" :size="14" /> New project</button>
    </div>

    <form v-if="adding" class="add" @submit.prevent="add">
      <input v-model="newName" placeholder="Name" maxlength="60" autocomplete="off" aria-label="Project name" />
      <input v-model="newFolder" placeholder="Folder (optional), e.g. C:\Projects\ledger" spellcheck="false" autocomplete="off" aria-label="Project folder" />
      <button class="primary" type="submit" :disabled="!newName.trim()">Add</button>
      <button type="button" class="quiet" @click="adding = false">Cancel</button>
    </form>

    <p v-if="!projects.list.length" class="empty">No projects yet.</p>
    <ul class="list">
      <li
        v-for="(p, i) in projects.list"
        :key="p.id"
        class="row"
        :class="{ dim: p.state !== 'active', over: over === p.id && dragging !== p.id, dragging: dragging === p.id }"
        :draggable="editing !== p.id"
        @dragstart="dragging = p.id"
        @dragend="dragging = null; over = null"
        @dragover.prevent="over = p.id"
        @drop.prevent="drop(p.id)"
      >
        <span class="grip" title="Drag to reorder"><Icon name="grip" :size="16" /></span>
        <div class="colour">
          <button class="swatch" :style="{ background: p.colour }" :title="`Colour ${p.colour}`" :aria-label="`Change ${p.name}'s colour`" @click="colouring = colouring === p.id ? null : p.id" />
          <div v-if="colouring === p.id" class="palette" role="listbox" aria-label="Colours">
            <button
              v-for="c in palette"
              :key="c"
              class="swatch"
              :class="{ on: c === p.colour }"
              :style="{ background: c }"
              :aria-label="c"
              role="option"
              :aria-selected="c === p.colour"
              @click="recolour(p, c)"
            />
          </div>
        </div>

        <form v-if="editing === p.id" class="edit" @submit.prevent="saveEdit(p)">
          <input v-model="draftName" maxlength="60" aria-label="Name" autocomplete="off" />
          <input v-model="draftFolder" placeholder="Folder (optional)" aria-label="Folder" spellcheck="false" autocomplete="off" />
          <button class="primary" type="submit" :disabled="!draftName.trim()">Save</button>
          <button type="button" class="quiet" @click="editing = null">Cancel</button>
        </form>
        <div v-else class="info" @dblclick="edit(p)">
          <p class="name">
            {{ p.name }}
            <span v-if="p.state !== 'active'" class="state">{{ STATE_LABELS[p.state] }}</span>
          </p>
          <p class="folder">{{ p.folder ?? 'No folder: sessions join it only when you choose it' }}</p>
        </div>

        <span class="count" :title="`${sessionCount(p.id)} open session(s)`">{{ sessionCount(p.id) || '' }}</span>

        <div v-if="editing !== p.id" class="actions">
          <template v-if="confirmingDelete === p.id">
            <span class="ask">Delete {{ p.name }}?</span>
            <button class="danger" @click="remove(p)">Delete</button>
            <button class="quiet" @click="confirmingDelete = null">Keep</button>
          </template>
          <template v-else>
            <button class="quiet icon" title="Move up" aria-label="Move up" :disabled="i === 0" @click="nudge(p, -1)"><Icon name="chevronDown" :size="15" style="transform: rotate(180deg)" /></button>
            <button class="quiet icon" title="Move down" aria-label="Move down" :disabled="i === projects.list.length - 1" @click="nudge(p, 1)"><Icon name="chevronDown" :size="15" /></button>
            <button class="quiet" @click="edit(p)">Edit</button>
            <button v-if="p.state === 'active'" class="quiet" title="Take it out of the rail; its sessions still show" @click="setState(p, 'hidden')">Hide</button>
            <button v-else class="quiet" @click="setState(p, 'active')">{{ p.state === 'archived' ? 'Restore' : 'Show' }}</button>
            <button v-if="p.state !== 'archived'" class="quiet" title="Finished with: out of the rail and the Overview's chips" @click="setState(p, 'archived')">Archive</button>
            <button class="quiet danger" @click="confirmingDelete = p.id">Delete</button>
          </template>
        </div>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.head {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 16px;
}
.head .primary {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  white-space: nowrap;
}
h2 {
  margin: 0 0 6px;
  font-size: 17px;
}
.hint {
  margin: 0 0 16px;
  color: var(--text-2);
  font-size: 13px;
  max-width: 64ch;
  line-height: 1.5;
}
.add,
.edit {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.add {
  margin-bottom: 16px;
}
.add input:first-child,
.edit input:first-child {
  width: 180px;
}
.add input:nth-child(2),
.edit input:nth-child(2) {
  flex: 1;
  min-width: 200px;
  font-family: var(--font-mono);
  font-size: 12.5px;
}
.empty {
  color: var(--text-muted);
}
.list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--surface);
  border: 1px solid var(--border);
}
.row.dim .info {
  opacity: 0.65;
}
.row.over {
  border-color: var(--accent);
}
.row.dragging {
  opacity: 0.5;
}
.grip {
  color: var(--text-muted);
  cursor: grab;
  display: grid;
}
.colour {
  position: relative;
}
.swatch {
  width: 18px;
  height: 18px;
  padding: 0;
  border-radius: 5px;
  border: 1px solid rgb(255 255 255 / 0.1);
}
.swatch.on {
  box-shadow: 0 0 0 2px var(--surface-raised), 0 0 0 4px var(--text);
}
.palette {
  position: absolute;
  top: 26px;
  left: -6px;
  z-index: 5;
  display: grid;
  grid-template-columns: repeat(5, 18px);
  gap: 8px;
  padding: 10px;
  border-radius: 10px;
  background: var(--surface-raised);
  border: 1px solid var(--border-strong);
  box-shadow: 0 10px 30px rgb(0 0 0 / 0.35);
}
.info {
  flex: 1;
  min-width: 0;
}
.edit {
  flex: 1;
}
.name {
  margin: 0;
  font-weight: 600;
  display: flex;
  gap: 8px;
  align-items: center;
}
.state {
  font-size: 11px;
  font-weight: 600;
  padding: 1px 7px;
  border-radius: var(--radius-pill);
  background: var(--surface-chip);
  color: var(--text-2);
}
.folder {
  margin: 2px 0 0;
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.count {
  color: var(--text-muted);
  font-size: 12px;
  min-width: 14px;
  text-align: right;
}
.actions {
  display: flex;
  align-items: center;
  gap: 2px;
}
.actions button {
  padding: 4px 9px;
  font-size: 12.5px;
}
.actions .icon {
  padding: 4px 5px;
  display: grid;
}
.ask {
  font-size: 12.5px;
  margin-right: 4px;
}
</style>
