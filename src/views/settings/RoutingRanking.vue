<script setup lang="ts">
/**
 * Settings → Routing, parts 2 and 3: the Data collection switch, which saves at once, and the
 * ranking settings, which are a draft with Save, Cancel and Restore defaults. Saved settings apply
 * to the next ranking (the inspector, New session and Refresh); they never change a running session
 * or a stored selection. Emits `saved` after Save, Restore defaults or the switch.
 */
import { computed, onMounted, ref } from 'vue'
import { commands } from '../../ipc'
import type { RoutingSettingsView } from '../../ipc/bindings'
import {
  DATA_COLLECTION_HELP,
  DATA_COLLECTION_LABEL,
  POLICY_CHOICES,
  RANKING_GROUPS,
  RESET_NOTICE,
  SAVED_NOTICE,
  actionStates,
  defaultText,
  isDirty,
  saveDataCollection,
  saveDraft,
  settle,
  switchBlocked,
  toDraft,
  type Draft,
  type RankingKey,
} from './routingRanking'

const emit = defineEmits<{ saved: [] }>()

const view = ref<RoutingSettingsView | null>(null)
const draft = ref<Draft | null>(null)
const loadError = ref<string | null>(null)
const saveError = ref<string | null>(null)
const switchError = ref<string | null>(null)
const notice = ref<string | null>(null)
const busy = ref(false)

function adopt(v: RoutingSettingsView): void {
  view.value = v
  draft.value = toDraft(v.settings)
  loadError.value = null
}

async function load(): Promise<void> {
  const r = await commands.routingSettings().catch((e) => ({ status: 'error' as const, error: String(e) }))
  if (r.status === 'error') loadError.value = r.error
  else adopt(r.data)
}
onMounted(() => void load())

const blocked = computed(() => switchBlocked(view.value))
const allowed = computed(() => view.value?.settings.dataCollection === 'allow')
const dirty = computed(() => (view.value && draft.value ? isDirty(draft.value, view.value.settings) : false))
const actions = computed(() => actionStates(dirty.value, busy.value))
const switchTitle = computed(() => blocked.value ?? (busy.value ? 'Saving…' : undefined))

async function flip(event: Event): Promise<void> {
  const box = event.target as HTMLInputElement
  if (!view.value) return
  busy.value = true
  switchError.value = null
  const r = await saveDataCollection(view.value.settings, box.checked, commands.routingSettingsSave)
  busy.value = false
  if (!r.ok) {
    switchError.value = r.reason
    box.checked = allowed.value
    return
  }
  // Only the switch changed: edits in the fields below stay as typed.
  view.value = r.view
  emit('saved')
}

async function save(): Promise<void> {
  if (!view.value || !draft.value) return
  busy.value = true
  saveError.value = null
  notice.value = null
  const r = await saveDraft(draft.value, view.value.settings, commands.routingSettingsSave)
  busy.value = false
  if (!r.ok) {
    saveError.value = r.reason
    return
  }
  adopt(r.view)
  notice.value = SAVED_NOTICE
  emit('saved')
}

function edit(key: RankingKey, event: Event): void {
  if (draft.value) draft.value[key] = (event.target as HTMLInputElement | HTMLSelectElement).value
}

function cancel(): void {
  if (!view.value) return
  draft.value = toDraft(view.value.settings)
  saveError.value = null
  notice.value = null
}

async function restore(): Promise<void> {
  busy.value = true
  saveError.value = null
  switchError.value = null
  notice.value = null
  const r = await settle(commands.routingSettingsReset())
  busy.value = false
  if (!r.ok) {
    saveError.value = r.reason
    return
  }
  adopt(r.view)
  notice.value = RESET_NOTICE
  emit('saved')
}
</script>

<template>
  <div class="parts">
    <section class="part">
      <h3>Data collection</h3>
      <label class="switch" :title="switchTitle">
        <input type="checkbox" role="switch" :checked="allowed" :disabled="!view || busy || !!blocked" :title="switchTitle" @change="flip" />
        <span>{{ DATA_COLLECTION_LABEL }}</span>
      </label>
      <p class="small">{{ DATA_COLLECTION_HELP }}</p>
      <p v-if="view?.problem" class="small warn">{{ view.problem }}</p>
      <p v-if="switchError" class="small bad" role="alert">{{ switchError }}</p>
      <p v-if="loadError" class="small bad" role="alert">
        The ranking settings couldn’t be loaded: {{ loadError }}
        <button type="button" class="link" @click="load">Try again</button>
      </p>
    </section>

    <section v-if="view && draft" class="part">
      <details class="fold">
        <summary>Ranking settings</summary>
        <div v-for="g in RANKING_GROUPS" :key="g.key" class="group">
          <h4>{{ g.heading }}</h4>
          <p v-if="g.note" class="small">{{ g.note }}</p>
          <div v-for="f in g.fields" :key="f.key" class="field">
            <label :for="`rank-${f.key}`">{{ f.short }}</label>
            <select v-if="f.key === 'unknownQuantPolicy'" :id="`rank-${f.key}`" :value="draft[f.key]" :disabled="busy" :title="busy ? 'Working…' : undefined" @change="edit(f.key, $event)">
              <option v-for="c in POLICY_CHOICES" :key="c.value" :value="c.value">{{ c.label }}</option>
            </select>
            <span v-else class="number">
              <input :id="`rank-${f.key}`" :value="draft[f.key]" type="text" inputmode="decimal" autocomplete="off" spellcheck="false" :disabled="busy" :title="busy ? 'Working…' : undefined" @input="edit(f.key, $event)" />
              <span v-if="f.unit" class="unit">{{ f.unit }}</span>
            </span>
            <span class="default">Default: {{ defaultText(f) }}</span>
            <p v-if="f.help" class="small help">{{ f.help }}</p>
          </div>
        </div>

        <div class="actions">
          <button type="button" class="primary" :disabled="actions.save.disabled" :title="actions.save.title" @click="save">Save</button>
          <button type="button" :disabled="actions.cancel.disabled" :title="actions.cancel.title" @click="cancel">Cancel</button>
          <button type="button" class="quiet" :disabled="actions.restore.disabled" :title="actions.restore.title" @click="restore">Restore defaults</button>
        </div>
        <p v-if="saveError" class="small bad" role="alert">{{ saveError }}</p>
        <p v-else-if="notice" class="small" role="status">{{ notice }}</p>
      </details>
    </section>
  </div>
</template>

<style scoped>
.parts {
  display: flex;
  flex-direction: column;
  gap: 28px;
}
.part {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
h3 {
  margin: 0;
  font-size: 15px;
}
h4 {
  margin: 0;
  font-size: 13px;
}
.switch {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  font-size: 13.5px;
  color: var(--text-2);
  max-width: 60ch;
  line-height: 1.5;
}
.switch input {
  margin-top: 3px;
}
.small {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.5;
  max-width: 70ch;
}
.warn {
  color: var(--status-needs);
}
.bad {
  color: var(--status-failed);
}
.link {
  padding: 0;
  border: none;
  background: none;
  color: var(--accent);
  font-size: 12px;
}
.fold > summary {
  cursor: pointer;
  font-size: 15px;
  font-weight: 600;
}
.group {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 16px;
}
.field {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 220px 190px;
  align-items: center;
  gap: 4px 16px;
  padding: 2px 0;
  font-size: 12.5px;
}
.field label {
  color: var(--text-2);
}
.number {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 44px;
  align-items: center;
  gap: 8px;
}
.number input,
.field select {
  width: 100%;
  box-sizing: border-box;
}
.field select,
.field input {
  padding: 6px 10px;
}
.unit,
.default {
  color: var(--text-muted);
}
.default {
  line-height: 1.35;
}
.help {
  grid-column: 1 / -1;
}
.actions {
  display: flex;
  gap: 10px;
  margin: 18px 0 8px;
}
.actions button {
  padding: 7px 14px;
}
</style>
