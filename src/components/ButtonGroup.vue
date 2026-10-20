<script setup lang="ts" generic="T extends string | number | null">
/**
 * A segmented control: one choice from a few, all in view (New session's agent, start, effort and
 * permissions), as in Chorus 1. Arrow keys move the choice, as in a radio group. With `clearable`,
 * clicking the chosen option clears it to `null` (an option with no default).
 */
import { computed } from 'vue'
import type { Choice } from './choice'

const props = defineProps<{ options: Choice<T>[]; modelValue: T; label: string; clearable?: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [value: T] }>()

// The chosen option takes Tab; with none chosen, the first enabled one does.
const tabStop = computed(() => {
  const i = props.options.findIndex((o) => o.value === props.modelValue)
  return i >= 0 ? i : props.options.findIndex((o) => !o.disabled)
})

function pick(o: Choice<T>): void {
  if (o.disabled) return
  emit('update:modelValue', props.clearable && o.value === props.modelValue ? (null as T) : o.value)
}

function onKey(e: KeyboardEvent, i: number): void {
  const step = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0
  if (!step) return
  e.preventDefault()
  const n = props.options.length
  for (let k = 1; k <= n; k++) {
    const j = (i + step * k + n) % n
    const o = props.options[j]!
    if (!o.disabled) {
      emit('update:modelValue', o.value)
      const group = (e.currentTarget as HTMLElement).parentElement
      ;(group?.children[j] as HTMLElement | undefined)?.focus()
      return
    }
  }
}
</script>

<template>
  <div class="group" role="radiogroup" :aria-label="label">
    <button
      v-for="(o, i) in options"
      :key="String(o.value)"
      type="button"
      role="radio"
      :aria-checked="o.value === modelValue"
      :class="{ on: o.value === modelValue }"
      :disabled="o.disabled"
      :title="o.title"
      :tabindex="i === tabStop ? 0 : -1"
      @click="pick(o)"
      @keydown="onKey($event, i)"
    >
      <slot name="option" :choice="o">{{ o.label }}</slot>
    </button>
  </div>
</template>

<style scoped>
.group {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  padding: 3px;
  border-radius: calc(var(--radius-control) + 3px);
  background: var(--surface);
  border: 1px solid var(--border);
}
button {
  flex: 1 1 auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 7px;
  padding: 6px 12px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-2);
  font-size: 13px;
  white-space: nowrap;
}
button:hover:not(:disabled) {
  color: var(--text);
  border-color: transparent;
  background: var(--surface-chip);
}
button.on,
button.on:hover:not(:disabled) {
  background: var(--accent-soft);
  border-color: var(--accent-line);
  color: var(--text);
  font-weight: 600;
}
</style>
