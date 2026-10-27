<script setup lang="ts">
/**
 * Add or edit a project from the rail: its name, its folder and the colour of its icon. Editing also
 * offers Delete, which asks first and says what happens to the project's sessions. Hide and the
 * order are on the rail itself.
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import Icon from '../components/Icon.vue'
import { commands } from '../ipc'
import { useProjectsStore } from '../stores/projects'
import { useSessionsStore } from '../stores/sessions'
import { useNoticesStore } from '../stores/notices'
import { useViewStore } from '../stores/view'

const props = defineProps<{ id: number | null; deleting?: boolean }>()
const emit = defineEmits<{ close: [] }>()

const projects = useProjectsStore()
const sessions = useSessionsStore()
const notices = useNoticesStore()
const view = useViewStore()

const project = computed(() => (props.id === null ? null : (projects.byId.get(props.id) ?? null)))
const dialog = ref<HTMLDialogElement | null>(null)
const nameInput = ref<HTMLInputElement | null>(null)
// Focus goes back where it was when the dialog closes.
const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null

const name = ref(project.value?.name ?? '')
const folder = ref(project.value?.folder ?? '')
const palette = ref<string[]>([])
// A new project starts on the colour the back end would hand out next.
const colour = ref(project.value?.colour ?? '')
const saving = ref(false)
const picking = ref(false)
const confirmingDelete = ref(props.deleting === true && project.value !== null)

const sessionCount = computed(() => (props.id === null ? 0 : sessions.list.filter((s) => s.projectId === props.id).length))
const canSave = computed(() => !!name.value.trim() && !!folder.value.trim() && !!colour.value && !saving.value && !picking.value)

onMounted(async () => {
  dialog.value?.showModal()
  if (!confirmingDelete.value) nameInput.value?.focus()
  palette.value = await commands.projectPalette().catch(() => [])
  if (!colour.value) colour.value = palette.value[projects.list.length % Math.max(palette.value.length, 1)] ?? '#8a97a6'
})
onUnmounted(() => {
  if (opener?.isConnected) opener.focus()
})

function dismiss(): void {
  if (saving.value) return
  dialog.value?.close()
  emit('close')
}

async function browse(): Promise<void> {
  if (picking.value) return
  picking.value = true
  try {
    const chosen = await commands.folderPick(folder.value.trim() || null)
    if (chosen !== null) folder.value = chosen
  } catch (e) {
    notices.error("Couldn't open the folder picker.", e)
  } finally {
    picking.value = false
  }
}

async function save(): Promise<void> {
  if (!canSave.value) return
  saving.value = true
  try {
    const input = { name: name.value, folder: folder.value.trim(), colour: colour.value }
    if (await projects.save(props.id, input)) {
      notices.success(props.id === null ? `Added ${name.value.trim()}.` : `Saved ${name.value.trim()}.`)
      dialog.value?.close()
      emit('close')
    }
  } finally {
    saving.value = false
  }
}

async function remove(): Promise<void> {
  const p = project.value
  if (!p || saving.value) return
  saving.value = true
  try {
    if (await projects.remove(p.id)) {
      if (view.projectFilter === p.id) view.openProject(null)
      notices.info(`Deleted ${p.name}. Its sessions keep running, in no project.`)
      dialog.value?.close()
      emit('close')
    }
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Teleport to="body">
    <dialog ref="dialog" class="project-dialog" aria-labelledby="project-dialog-title" @cancel.prevent="dismiss">
      <h2 id="project-dialog-title">{{ confirmingDelete ? `Delete ${project?.name}?` : id === null ? 'New project' : 'Edit project' }}</h2>

      <template v-if="confirmingDelete && project">
        <p class="what">
          This removes the project from Chorus.
          <template v-if="sessionCount">
            Its {{ sessionCount === 1 ? 'session keeps' : `${sessionCount} sessions keep` }} running, in no project.
          </template>
          <template v-else>It has no open sessions.</template>
          Nothing in <code>{{ project.folder }}</code> is touched. To keep it but tidy the sidebar, hide it instead.
        </p>
        <footer>
          <button class="quiet" :disabled="saving" autofocus @click="confirmingDelete = false">Keep</button>
          <button class="danger" :disabled="saving" @click="remove">Delete project</button>
        </footer>
      </template>

      <form v-else @submit.prevent="save">
        <label class="field">
          <span>Name</span>
          <input ref="nameInput" v-model="name" required maxlength="60" autocomplete="off" :disabled="saving" />
        </label>

        <div class="field">
          <label for="project-folder">Folder</label>
          <div class="folder-field">
            <input id="project-folder" v-model="folder" required maxlength="1024" spellcheck="false" autocomplete="off" placeholder="Where its sessions work" :disabled="saving" />
            <button type="button" class="browse" :disabled="picking || saving" @click="browse"><Icon name="folder" :size="16" /> Browse</button>
          </div>
          <small>A new session whose folder is inside this one joins the project.</small>
        </div>

        <div class="field">
          <span id="project-colour-label">Icon colour</span>
          <div class="colours" role="radiogroup" aria-labelledby="project-colour-label">
            <button
              v-for="c in palette"
              :key="c"
              type="button"
              class="swatch"
              :class="{ on: c === colour }"
              :style="{ background: c }"
              role="radio"
              :aria-checked="c === colour"
              :aria-label="c"
              :disabled="saving"
              @click="colour = c"
            />
            <label class="custom" title="Pick any colour">
              <input v-model="colour" type="color" :disabled="saving" aria-label="Custom colour" />
              <span>Custom</span>
            </label>
          </div>
        </div>

        <footer>
          <button v-if="project" type="button" class="quiet danger delete" :disabled="saving" @click="confirmingDelete = true">Delete…</button>
          <button type="button" class="quiet" :disabled="saving" @click="dismiss">Cancel</button>
          <button type="submit" class="primary" :disabled="!canSave">{{ id === null ? 'Add project' : 'Save' }}</button>
        </footer>
      </form>
    </dialog>
  </Teleport>
</template>

<style scoped>
.project-dialog {
  width: min(520px, calc(100vw - 48px));
  max-height: calc(100dvh - 64px);
  padding: 20px 22px 18px;
  border: 1px solid var(--border-strong);
  border-radius: 14px;
  background: var(--surface-raised);
  color: var(--text);
  box-shadow: 0 24px 80px #0008;
}
.project-dialog[open] {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.project-dialog::backdrop {
  background: rgb(0 0 0 / 0.5);
}
h2 {
  margin: 0;
  font-size: 18px;
}
form {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12.5px;
  font-weight: 600;
  color: var(--text-2);
}
.field small {
  font-weight: 400;
  color: var(--text-muted);
}
.field input:not([type='color']) {
  width: 100%;
}
.folder-field {
  display: flex;
  gap: 8px;
}
.folder-field input {
  flex: 1;
  min-width: 0;
  font-family: var(--font-mono);
  font-size: 12.5px;
}
.browse {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: none;
}
.colours {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
}
.swatch {
  width: 22px;
  height: 22px;
  padding: 0;
  border-radius: 6px;
  border: 1px solid rgb(255 255 255 / 0.12);
}
.swatch.on {
  box-shadow: 0 0 0 2px var(--surface-raised), 0 0 0 4px var(--text);
}
.custom {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin-left: 4px;
  font-weight: 500;
  cursor: pointer;
}
.custom input {
  width: 26px;
  height: 22px;
  padding: 0;
  border: 1px solid var(--border-strong);
  border-radius: 6px;
  background: transparent;
  cursor: pointer;
}
.what {
  margin: 0;
  font-size: 13px;
  line-height: 1.55;
  color: var(--text-2);
  overflow-wrap: anywhere;
}
footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.delete {
  margin-right: auto;
}
</style>
