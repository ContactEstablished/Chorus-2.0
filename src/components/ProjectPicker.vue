<script setup lang="ts">
/**
 * A session's project, picked by typing (D52): matching projects as you type, "By folder" (the
 * project whose folder holds the session's, if any), or a new project made on the spot with the
 * session's folder. Esc closes the list before it closes anything else.
 */
import { computed, ref } from 'vue'
import type { Project } from '../ipc'
import { useProjectsStore } from '../stores/projects'
import { suggest } from './choice'

const props = defineProps<{ modelValue: number | null; auto: Project | null; newFolder: string | null }>()
const emit = defineEmits<{ 'update:modelValue': [id: number | null] }>()
const projects = useProjectsStore()

const draft = ref('')
const open = ref(false)
const active = ref(0)
const busy = ref(false)
const listId = `projects-${Math.random().toString(36).slice(2, 8)}`

const chosen = computed(() => (props.modelValue === null ? null : (projects.byId.get(props.modelValue) ?? null)))
const autoText = computed(() => `By folder: ${props.auto?.name ?? 'no project'}`)

type Offer = { kind: 'auto' } | { kind: 'project'; project: Project } | { kind: 'new'; name: string }
const offers = computed<Offer[]>(() => {
  const q = draft.value.trim()
  const names = projects.usable.map((p) => p.name)
  const found = suggest(names, q, [], 12).map((n) => projects.usable.find((p) => p.name === n)!)
  const exact = projects.list.some((p) => p.name.toLowerCase() === q.toLowerCase())
  return [
    ...(q ? [] : [{ kind: 'auto' } as Offer]),
    ...found.map((project) => ({ kind: 'project', project }) as Offer),
    ...(q && !exact ? [{ kind: 'new', name: q } as Offer] : []),
  ]
})

async function take(o: Offer): Promise<void> {
  open.value = false
  draft.value = ''
  if (o.kind === 'auto') emit('update:modelValue', null)
  else if (o.kind === 'project') emit('update:modelValue', o.project.id)
  else {
    busy.value = true
    const ok = await projects.save(null, { name: o.name, folder: props.newFolder, colour: null })
    busy.value = false
    const made = ok ? projects.list.filter((p) => p.name === o.name).sort((a, b) => b.id - a.id)[0] : undefined
    if (made) emit('update:modelValue', made.id)
  }
}

function onKey(e: KeyboardEvent): void {
  const n = offers.value.length
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault()
    open.value = true
    if (n) active.value = (active.value + (e.key === 'ArrowDown' ? 1 : -1) + n) % n
  } else if (e.key === 'Enter' && open.value && n && (draft.value.trim() || active.value > 0)) {
    e.preventDefault()
    void take(offers.value[active.value]!)
  } else if (e.key === 'Escape' && open.value) {
    e.stopPropagation()
    open.value = false
    draft.value = ''
  }
}
</script>

<template>
  <div class="picker">
    <span class="swatch" :style="{ background: chosen?.colour ?? auto?.colour ?? 'transparent' }" :class="{ none: !chosen && !auto }" />
    <input
      v-model="draft"
      :placeholder="chosen ? chosen.name : autoText"
      :class="{ set: chosen }"
      aria-label="Project"
      role="combobox"
      :aria-expanded="open && offers.length > 0"
      :aria-controls="listId"
      :readonly="busy"
      autocomplete="off"
      spellcheck="false"
      maxlength="60"
      @focus="open = true; active = 0"
      @blur="open = false; draft = ''"
      @input="open = true; active = 0"
      @keydown="onKey"
    />
    <ul v-if="open && offers.length" :id="listId" class="offers" role="listbox">
      <li
        v-for="(o, i) in offers"
        :key="o.kind === 'project' ? o.project.id : o.kind"
        role="option"
        :aria-selected="i === active"
        :class="{ on: i === active }"
        @mousedown.prevent="take(o)"
        @mouseenter="active = i"
      >
        <template v-if="o.kind === 'auto'">
          <span class="swatch" :style="{ background: auto?.colour ?? 'transparent' }" :class="{ none: !auto }" />
          {{ autoText }}
        </template>
        <template v-else-if="o.kind === 'project'">
          <span class="swatch" :style="{ background: o.project.colour }" />
          {{ o.project.name }}
          <span v-if="o.project.folder" class="folder">{{ o.project.folder }}</span>
        </template>
        <template v-else>
          <span class="plus">+</span> New project “{{ o.name }}”<span v-if="newFolder" class="folder">{{ newFolder }}</span>
        </template>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.picker {
  position: relative;
  display: flex;
  align-items: center;
}
.picker > .swatch {
  position: absolute;
  left: 11px;
  pointer-events: none;
}
.swatch {
  flex: none;
  width: 9px;
  height: 9px;
  border-radius: 3px;
}
.swatch.none {
  border: 1px dashed var(--text-muted);
}
input {
  width: 100%;
  padding-left: 28px;
}
input.set::placeholder {
  color: var(--text);
  opacity: 1;
}
.offers {
  position: absolute;
  z-index: 5;
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  max-height: 260px;
  overflow: auto;
  margin: 0;
  padding: 4px;
  list-style: none;
  border-radius: var(--radius-control);
  border: 1px solid var(--border-strong);
  background: var(--surface-raised);
  box-shadow: 0 12px 30px rgb(0 0 0 / 0.35);
}
.offers li {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
}
.offers li.on {
  background: var(--accent-soft);
}
.folder {
  margin-left: auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-muted);
}
.plus {
  width: 9px;
  text-align: center;
  color: var(--accent);
  font-weight: 700;
}
</style>
