<script setup lang="ts" generic="T extends string | number | null">
/**
 * One choice from a few, all in view (New session's agent, mode, workspace, effort and
 * permissions). Arrow keys move the choice, as in a radio group. With `clearable`, clicking the
 * chosen option clears it to `null` (an option with no default). Variants, from the New session
 * design (1c): `segmented` (a pill bar), `tiles` (big tiles, content from the slot), `chips` (a row
 * of chips with a radio dot) and `radios` (a list with a radio dot and, when chosen, the hint).
 */
import { computed } from 'vue'
import type { Choice } from './choice'

const props = withDefaults(
  defineProps<{ options: Choice<T>[]; modelValue: T; label: string; clearable?: boolean; variant?: 'segmented' | 'tiles' | 'chips' | 'radios' }>(),
  { clearable: false, variant: 'segmented' },
)
const emit = defineEmits<{ 'update:modelValue': [value: T] }>()

// The chosen option takes Tab; with none chosen, the first enabled one does.
const tabStop = computed(() => {
  const i = props.options.findIndex((o) => o.value === props.modelValue)
  return i >= 0 ? i : props.options.findIndex((o) => !o.disabled)
})
const dotted = computed(() => props.variant === 'chips' || props.variant === 'radios')

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
  <div class="group" :class="variant" role="radiogroup" :aria-label="label">
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
      <span v-if="dotted" class="dot" />
      <slot name="option" :choice="o" :on="o.value === modelValue">
        <span class="text">{{ o.label }}</span>
      </slot>
      <span v-if="variant === 'radios' && o.hint && o.value === modelValue" class="hint">{{ o.hint }}</span>
    </button>
  </div>
</template>

<style scoped>
.group {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}
button {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-2);
  font-size: 13px;
  white-space: nowrap;
}
button:hover:not(:disabled) {
  color: var(--text);
}
button.on {
  color: var(--text);
  font-weight: 600;
}

/* A pill bar. */
.segmented {
  padding: 3px;
  border-radius: calc(var(--radius-control) + 1px);
  background: var(--surface);
  border: 1px solid var(--border);
}
.segmented button {
  flex: 1 1 auto;
  justify-content: center;
  padding: 5px 10px;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 600;
}
.segmented button:hover:not(:disabled) {
  border-color: transparent;
  background: var(--surface-chip);
}
.segmented button.on,
.segmented button.on:hover:not(:disabled) {
  background: var(--accent-soft);
}

/* Tiles: the slot draws them. */
.tiles {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(86px, 1fr));
  gap: 6px;
}
.tiles button {
  flex-direction: column;
  justify-content: center;
  gap: 7px;
  padding: 12px 6px 10px;
  border-radius: 10px;
  background: var(--surface);
  border-color: var(--border-strong);
  font-size: 12px;
}
.tiles button:hover:not(:disabled) {
  border-color: var(--accent-line);
}
.tiles button.on,
.tiles button.on:hover:not(:disabled) {
  background: var(--accent-soft);
  border-color: color-mix(in srgb, var(--accent) 75%, transparent);
}

/* Chips with a radio dot. */
.chips {
  gap: 6px;
}
.chips button {
  padding: 7px 12px;
  border-radius: 8px;
  background: var(--surface);
  border-color: var(--border-strong);
  font-size: 12px;
}
.chips button:hover:not(:disabled) {
  border-color: var(--accent-line);
}
.chips button.on,
.chips button.on:hover:not(:disabled) {
  background: var(--accent-soft);
  border-color: color-mix(in srgb, var(--accent) 75%, transparent);
}

/* A list with a radio dot. */
.radios {
  flex-direction: column;
  flex-wrap: nowrap;
  gap: 2px;
}
.radios button {
  padding: 6px 10px;
  border-radius: 8px;
  text-align: left;
}
.radios button:hover:not(:disabled) {
  background: var(--surface);
}
.radios button.on {
  background: var(--accent-soft);
}
.radios .text {
  flex: 1;
}

.dot {
  flex: none;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  border: 1.5px solid var(--border-strong);
  box-sizing: border-box;
}
.chips .dot {
  width: 12px;
  height: 12px;
}
button.on .dot {
  border: 4px solid var(--accent);
}
.chips button.on .dot {
  border-width: 3.5px;
}
.hint {
  font-family: var(--font-mono);
  font-size: 11px;
  font-weight: 400;
  color: var(--text-muted);
}
</style>
