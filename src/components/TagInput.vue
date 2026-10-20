<script setup lang="ts">
/**
 * Tags (D52): chips plus a field. Typing offers tags used before (most used first); Enter, Tab or a
 * comma adds the highlighted one, or a new tag made of what's typed. Backspace in an empty field
 * removes the last tag. Esc closes the suggestions before it closes anything else.
 */
import { computed, ref } from 'vue'
import { MAX_TAGS, cleanTag, suggest } from './choice'

const props = defineProps<{ modelValue: string[]; known: string[]; label?: string }>()
const emit = defineEmits<{ 'update:modelValue': [tags: string[]] }>()

const draft = ref('')
const open = ref(false)
const active = ref(0)
const field = ref<HTMLInputElement | null>(null)
const listId = `tags-${Math.random().toString(36).slice(2, 8)}`

const typed = computed(() => cleanTag(draft.value))
const offers = computed(() => {
  const found = suggest(props.known, typed.value, props.modelValue)
  const isNew = typed.value !== '' && !props.known.includes(typed.value) && !props.modelValue.includes(typed.value)
  return [...found.map((tag) => ({ tag, isNew: false })), ...(isNew ? [{ tag: typed.value, isNew: true }] : [])]
})
const full = computed(() => props.modelValue.length >= MAX_TAGS)

function add(tag: string): void {
  if (tag && !props.modelValue.includes(tag) && !full.value) emit('update:modelValue', [...props.modelValue, tag])
  draft.value = ''
  active.value = 0
  navigated.value = false
}
function remove(tag: string): void {
  emit('update:modelValue', props.modelValue.filter((t) => t !== tag))
  field.value?.focus()
}

// Arrowed to a suggestion: Enter takes it even with nothing typed.
const navigated = ref(false)

function onKey(e: KeyboardEvent): void {
  const shown = open.value && offers.value.length > 0
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault()
    open.value = true
    navigated.value = true
    const n = offers.value.length
    if (n) active.value = (active.value + (e.key === 'ArrowDown' ? 1 : -1) + n) % n
    return
  }
  // With nothing typed or chosen, Enter submits the form and Tab moves on.
  const takes = e.key === 'Enter' || e.key === ',' || (e.key === 'Tab' && typed.value !== '')
  if (takes && (typed.value || (shown && navigated.value))) {
    e.preventDefault()
    add(shown ? (offers.value[active.value]?.tag ?? typed.value) : typed.value)
  } else if (e.key === ',') {
    e.preventDefault()
  } else if (e.key === 'Backspace' && draft.value === '' && props.modelValue.length) {
    emit('update:modelValue', props.modelValue.slice(0, -1))
  } else if (e.key === 'Escape' && shown) {
    e.stopPropagation()
    open.value = false
  }
}
</script>

<template>
  <div class="tags" :class="{ focus: open }" @click="field?.focus()">
    <span v-for="t in modelValue" :key="t" class="tag">
      {{ t }}
      <button type="button" class="x" :aria-label="`Remove ${t}`" @click.stop="remove(t)">×</button>
    </span>
    <input
      ref="field"
      v-model="draft"
      :placeholder="modelValue.length ? '' : full ? '' : 'Add a tag…'"
      :disabled="full"
      :aria-label="label ?? 'Tags'"
      role="combobox"
      :aria-expanded="open && offers.length > 0"
      :aria-controls="listId"
      autocomplete="off"
      spellcheck="false"
      maxlength="40"
      @focus="open = true"
      @blur="open = false"
      @input="open = true; active = 0; navigated = false"
      @keydown="onKey"
    />
    <ul v-if="open && offers.length" :id="listId" class="offers" role="listbox">
      <li
        v-for="(o, i) in offers"
        :key="o.tag + o.isNew"
        role="option"
        :aria-selected="i === active"
        :class="{ on: i === active }"
        @mousedown.prevent="add(o.tag)"
        @mouseenter="active = i"
      >
        <template v-if="o.isNew">New tag “{{ o.tag }}”</template>
        <template v-else>{{ o.tag }}</template>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.tags {
  position: relative;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  min-height: 36px;
  padding: 4px 6px;
  border-radius: var(--radius-control);
  border: 1px solid var(--border-strong);
  background: var(--surface);
  cursor: text;
}
.tags.focus {
  outline: 2px solid var(--accent-line);
  outline-offset: 1px;
}
.tag {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 2px 4px 2px 9px;
  border-radius: var(--radius-pill);
  background: var(--surface-chip);
  border: 1px solid var(--border-strong);
  font-size: 12.5px;
}
.x {
  padding: 0 5px;
  border: 0;
  background: transparent;
  color: var(--text-muted);
  font-size: 14px;
  line-height: 1;
}
.x:hover:not(:disabled) {
  color: var(--text);
}
input {
  flex: 1;
  min-width: 90px;
  border: 0;
  padding: 4px;
  background: transparent;
}
input:focus {
  outline: none;
}
.offers {
  position: absolute;
  z-index: 5;
  top: calc(100% + 4px);
  left: 0;
  min-width: 200px;
  max-height: 240px;
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
  padding: 6px 10px;
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
}
.offers li.on {
  background: var(--accent-soft);
}
</style>
