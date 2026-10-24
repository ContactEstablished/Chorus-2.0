<script setup lang="ts">
/**
 * Raw view search (P7): finds text in the session's terminal buffer, scrollback included, with
 * next, previous and a match count. Opened by its button or Ctrl+Shift+F in the terminal; Enter
 * finds the next match, Shift+Enter the previous, Escape closes and returns to the terminal.
 */
import { computed, onMounted, ref, watch } from 'vue'
import Icon from '../components/Icon.vue'
import { matchCount } from './search'

interface Searchable {
  find: (text: string, backwards?: boolean, caseSensitive?: boolean, typing?: boolean) => boolean
  clearSearch: () => void
  results: () => { index: number; count: number } | null
  focus: () => void
}

const props = defineProps<{ term: Searchable | null }>()
const emit = defineEmits<{ close: [] }>()

const text = ref('')
const caseSensitive = ref(false)
const input = ref<HTMLInputElement | null>(null)
// The addon reports results through an event, after the search; the terminal keeps them reactive.
const results = computed(() => props.term?.results() ?? null)

const count = computed(() => matchCount(text.value, results.value))

function find(backwards = false, typing = false): void {
  props.term?.find(text.value, backwards, caseSensitive.value, typing)
}

function close(): void {
  props.term?.clearSearch()
  emit('close')
  props.term?.focus()
}

function onKey(e: KeyboardEvent): void {
  if (e.key === 'Enter') {
    e.preventDefault()
    find(e.shiftKey)
  } else if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    close()
  }
}

// Typing refines the current match; Enter and the arrows move to the next or previous.
watch([text, caseSensitive], () => find(false, true))
onMounted(() => input.value?.focus())
defineExpose({ focus: () => input.value?.focus() })
</script>

<template>
  <div class="search" role="search">
    <Icon name="search" :size="14" />
    <input
      ref="input"
      v-model="text"
      class="field"
      type="search"
      placeholder="Find in terminal"
      aria-label="Find in terminal"
      spellcheck="false"
      @keydown="onKey"
    />
    <span class="count" aria-live="polite">{{ count }}</span>
    <label class="case" title="Match case"><input v-model="caseSensitive" type="checkbox" /> Aa</label>
    <button class="quiet" title="Previous match (Shift+Enter)" aria-label="Previous match" :disabled="!text" @click="find(true)">↑</button>
    <button class="quiet" title="Next match (Enter)" aria-label="Next match" :disabled="!text" @click="find()">↓</button>
    <span class="scope">Searches the last 10,000 lines</span>
    <button class="quiet" title="Close (Esc)" aria-label="Close search" @click="close"><Icon name="close" :size="14" /></button>
  </div>
</template>

<style scoped>
.search {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 8px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-control);
  background: var(--surface-raised);
  font-size: 12px;
  color: var(--text-2);
}
.field {
  flex: 1;
  min-width: 120px;
  max-width: 320px;
  border: 0;
  background: transparent;
  color: var(--text);
  font: inherit;
  outline: none;
}
.count {
  min-width: 72px;
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
}
.case {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  cursor: pointer;
}
.scope {
  margin-left: auto;
  color: var(--text-muted);
}
</style>
