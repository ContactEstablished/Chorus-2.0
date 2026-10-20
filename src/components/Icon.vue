<script setup lang="ts">
/** Line icons on a 24-unit grid, drawn in the current text colour. Drawn here, not from a package. */
import { computed } from 'vue'
import { PATHS, type IconName } from './icons'

const props = withDefaults(defineProps<{ name: IconName; size?: number; stroke?: number }>(), { size: 18, stroke: 1.75 })
const d = computed(() => PATHS[props.name])
</script>

<template>
  <svg
    class="icon"
    :width="props.size"
    :height="props.size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    :stroke-width="props.stroke"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    focusable="false"
  >
    <path :d="d" />
  </svg>
</template>

<style scoped>
.icon {
  flex: none;
}
</style>
