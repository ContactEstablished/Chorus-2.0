<script setup lang="ts">
/**
 * The seven-bar Chorus mark (DESIGN.md "Logo", D24). Seven bars 6 px wide on a 13 px pitch,
 * fully rounded, vertically centred in an 84 x 76 box; heights 28/44/62/76/62/44/28. The centre
 * bar is the only coloured one. The symmetry is the mark: never stretch or reorder it.
 */
import { computed } from 'vue'

const props = withDefaults(defineProps<{ height?: number }>(), { height: 24 })

const VIEWBOX_W = 84
const VIEWBOX_H = 76
const HEIGHTS = [28, 44, 62, 76, 62, 44, 28] as const
const TONES = ['low', 'mid', 'high', 'lead', 'high', 'mid', 'low'] as const
const FILL = {
  low: 'var(--logo-bar-low)',
  mid: 'var(--logo-bar-mid)',
  high: 'var(--logo-bar-high)',
  lead: 'var(--accent)',
} as const

const bars = HEIGHTS.map((h, i) => ({ x: i * 13, y: (VIEWBOX_H - h) / 2, h, fill: FILL[TONES[i] ?? 'low'] }))
const width = computed(() => (props.height * VIEWBOX_W) / VIEWBOX_H)
</script>

<template>
  <svg
    class="chorus-mark"
    :width="width"
    :height="props.height"
    :viewBox="`0 0 ${VIEWBOX_W} ${VIEWBOX_H}`"
    aria-hidden="true"
    focusable="false"
  >
    <rect v-for="bar in bars" :key="bar.x" :x="bar.x" :y="bar.y" width="6" :height="bar.h" rx="3" :fill="bar.fill" />
  </svg>
</template>

<style scoped>
.chorus-mark {
  flex: none;
}
</style>
