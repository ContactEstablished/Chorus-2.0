<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { commands, unwrap } from '../../ipc'
import { useKeysStore } from '../../stores/keys'
import { useProjectsStore } from '../../stores/projects'

const keys = useKeysStore()
const projects = useProjectsStore()
const credential = ref<number | null>(null)
const model = ref('')
const projectEnabled = ref<Record<number, boolean>>({})
const error = ref<string | null>(null)
const saved = ref(false)
const busy = ref(false)
onMounted(async () => {
  try {
    const config = await unwrap(commands.narratorConfig())
    credential.value = config.credentialId
    model.value = config.model
    await Promise.all([keys.load(), projects.load()])
    for (const project of projects.list) projectEnabled.value[project.id] = await unwrap(commands.narratorProjectEnabled(project.id))
  } catch (e) { error.value = String(e) }
})
async function save(): Promise<void> {
  busy.value = true; error.value = null; saved.value = false
  try { await unwrap(commands.narratorConfigSave({ credentialId: credential.value, model: model.value.trim() })); saved.value = true }
  catch (e) { error.value = String(e) }
  finally { busy.value = false }
}
async function toggle(id: number, enabled: boolean): Promise<void> {
  busy.value = true; error.value = null
  try { await unwrap(commands.narratorProjectSet(id, enabled)); projectEnabled.value[id] = enabled }
  catch (e) { error.value = String(e) }
  finally { busy.value = false }
}
</script>

<template>
  <section class="narrator-settings">
    <h2>Narrator</h2>
    <p>Summarise each completed turn into Now, Blocker, Up next and suggested drafts. The narrator sends scrubbed excerpts of your prompt, final message, plan, tool summaries, diff counts and resolved suggestions to OpenRouter.</p>
    <p>Choose a low-cost OpenRouter model that supports JSON output. One request per new completed turn may use API credit. Failed or interrupted requests are never retried; old turns are never sent when Chorus starts.</p>
    <form @submit.prevent="save">
      <label>OpenRouter API key
        <select v-model="credential"><option :value="null">No key — generic Continue and Review</option><option v-for="key in keys.keys" :key="key.id" :value="key.id">{{ key.label }}</option></select>
      </label>
      <p class="hint">Add a key under Keys and lock. Use an OpenRouter API key, not its management key.</p>
      <label>OpenRouter model ID<input v-model="model" maxlength="200" placeholder="provider/model" autocomplete="off" /></label>
      <button type="submit" :disabled="busy">Save narrator settings</button>
      <span v-if="saved" role="status">Saved. Applies to future completed turns.</span>
    </form>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <h3>Projects</h3>
    <p class="hint">On by default. Turning a project off keeps its history and uses generic drafts. An already sent request may still finish.</p>
    <label v-for="project in projects.list" :key="project.id" class="project">
      <input type="checkbox" :checked="projectEnabled[project.id] !== false" :disabled="busy" @change="toggle(project.id, ($event.target as HTMLInputElement).checked)" />
      {{ project.name }}
    </label>
    <p v-if="!projects.list.length" class="hint">Add a project to control its narrator separately.</p>
  </section>
</template>

<style scoped>
.narrator-settings { display: grid; gap: 16px; }
h2,h3,p { margin: 0; }
p { color: var(--text-2); line-height: 1.5; font-size: 13px; }
form { display: grid; gap: 14px; justify-items: start; }
label { display: grid; gap: 7px; width: 100%; font-size: 13px; }
select,input:not([type=checkbox]) { width: 100%; }
.hint { font-size: 12px; color: var(--text-muted); }
.error { color: var(--status-failed); }
.project { display: flex; align-items: center; }
</style>
