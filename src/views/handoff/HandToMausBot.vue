<script setup lang="ts">
// P18 (18-2): handing a project to OpenMausBot. Chorus makes a `maus/…` worktree from the
// project's last commit, writes the hand-off entry, and starts the conversation in MausBot when it
// is paired with full access; otherwise it gives Matthew the message to start it himself.
import { computed, onMounted, ref } from 'vue'
import type { MausBots, Project } from '../../ipc'
import { useHandoffStore } from '../../stores/handoff'

const props = defineProps<{ project: Project }>()
const emit = defineEmits<{ close: [] }>()
const store = useHandoffStore()

const bots = ref<MausBots | null>(null)
const loading = ref(true)
const botId = ref<string>('')
const task = ref('')
const error = ref<string | null>(null)
const busy = computed(() => store.busy.has(props.project.id))

onMounted(async () => {
  try {
    bots.value = await store.bots(props.project.id)
    botId.value = bots.value.defaultId ?? ''
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
  }
})

/** Why this hand-off will start by hand, if it will. */
const byHand = computed(() => {
  const b = bots.value
  if (!b) return null
  if (!b.running) return 'MausBot isn’t running, so Chorus will make the branch and give you the message to start it by hand.'
  if (!botId.value) return 'With no bot chosen, Chorus makes the branch and gives you the message to start it by hand.'
  if (!b.paired) return 'Chorus isn’t paired with MausBot (Settings → MausBot), so you’ll start the conversation by hand.'
  if (!b.canChooseFolder) return 'Chorus’s pairing is chat-only, so it can’t choose the folder: you’ll start the conversation by hand.'
  return null
})

async function start(): Promise<void> {
  error.value = null
  const ok = await store.start(props.project.id, botId.value || null, task.value.trim())
  if (ok) emit('close')
  else error.value = store.errors.get(props.project.id) ?? 'Chorus couldn’t hand the project over.'
}
</script>

<template>
  <section class="card" aria-label="Hand to MausBot">
    <header class="head">
      <h3>Hand to MausBot</h3>
    </header>
    <p class="lead">
      MausBot gets a branch of its own, in a worktree beside the project, made from the last commit. Your checkout and its
      uncommitted files stay as they are.
    </p>
    <label class="field">
      <span>Bot</span>
      <select v-model="botId" :disabled="loading || busy">
        <option v-for="b in bots?.bots ?? []" :key="b.id" :value="b.id">
          {{ b.name }}{{ b.title ? ` — ${b.title}` : '' }}{{ b.folderMatches ? ' (this project’s folder)' : '' }}{{ b.busy ? ' · working' : '' }}
        </option>
        <option value="">Start it by hand</option>
      </select>
    </label>
    <p v-if="bots?.problem" class="warn">{{ bots.problem }}</p>
    <label class="field">
      <span>Task <em>optional</em></span>
      <input v-model="task" type="text" maxlength="200" :disabled="busy" placeholder="What MausBot should do" @keydown.enter.prevent="start" />
    </label>
    <p v-if="byHand" class="muted">{{ byHand }}</p>
    <p v-if="error" class="warn" role="alert">{{ error }}</p>
    <div class="actions">
      <button class="primary" :disabled="busy || loading" @click="start">{{ busy ? 'Handing over…' : 'Hand over' }}</button>
      <button class="quiet" :disabled="busy" @click="emit('close')">Cancel</button>
    </div>
  </section>
</template>

<style scoped>
.card { background: var(--surface); border: 1px solid var(--accent-line); border-radius: var(--radius-card); padding: 12px 16px 14px; display: grid; gap: 10px; }
.head h3 { margin: 0; font-size: 13px; font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; color: var(--text-2); }
.lead { margin: 0; font-size: 13px; color: var(--text-2); }
.field { display: grid; gap: 4px; font-size: 12.5px; color: var(--text-muted); }
.field em { font-style: normal; color: var(--text-muted); opacity: 0.8; }
.field select, .field input { font-size: 13px; }
.muted { margin: 0; font-size: 12.5px; color: var(--text-muted); }
.warn { margin: 0; font-size: 12.5px; color: var(--status-needs); overflow-wrap: anywhere; }
.actions { display: flex; gap: 8px; }
</style>
