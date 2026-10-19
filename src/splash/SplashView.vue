<script setup lang="ts">
/**
 * The startup splash window (spike S7; built for real in P4). It runs beside session restore and
 * never holds it up: the back end restores sessions whatever the splash is doing, and this window
 * only reflects progress. Any key or click skips it. Reduced-motion users get a short static frame.
 *
 * In the spike, restore progress is simulated (`?sessions=N`); P4 feeds it from restore events.
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import ChorusMark from '../components/ChorusMark.vue'
import type { Splash, SplashMetrics } from './scene'

const params = new URLSearchParams(window.location.search)
const total = Math.max(0, Math.min(24, Number(params.get('sessions') ?? 6)))
const reduced = params.get('motion') === 'reduce' || window.matchMedia('(prefers-reduced-motion: reduce)').matches
const PROJECT_COLOURS = ['#1ec8f0', '#f472b6', '#e0a96d', '#a5b4fc', '#c4e05a', '#00e676', '#ffc400', '#4d7cff']

const canvas = ref<HTMLCanvasElement | null>(null)
const restored = ref(0)
const fading = ref(false)
const label = computed(() =>
  total === 0 ? 'Starting' : restored.value < total ? `Restoring ${restored.value} of ${total} sessions` : `${total} sessions restored`,
)

// Simulated restore times, spread over the first ~2 s (P4: real restore events).
const restoredAtMs = Array.from({ length: total }, (_, i) => 350 + Math.round((1700 * (i + 0.5)) / Math.max(1, total)))
const timers: number[] = restoredAtMs.map((at) => window.setTimeout(() => restored.value++, at))

let splash: Splash | null = null
const report: { metrics: SplashMetrics | null; reduced: boolean; navToScriptMs: number; timeOrigin: number } = {
  metrics: null,
  reduced,
  navToScriptMs: Math.round(performance.now()),
  timeOrigin: performance.timeOrigin,
}
;(window as unknown as { __splash: typeof report }).__splash = report

/** Spike A/B switches left in shared storage by the harness. */
function variant(): { bloom?: boolean; toneMapping?: boolean } {
  try {
    return JSON.parse(localStorage.getItem('chorus.spike.splash.variant') ?? '{}')
  } catch {
    return {}
  }
}

function done(metrics: SplashMetrics | null): void {
  report.metrics = metrics
  // Spike: leave the measurements where the main window's harness can read them (same origin).
  try {
    localStorage.setItem('chorus.spike.splash', JSON.stringify(report))
  } catch {
    /* storage unavailable */
  }
  window.setTimeout(() => void getCurrentWindow().close(), 50)
}

function skip(): void {
  if (splash) splash.skip()
  else if (reduced) done(null)
}

onMounted(async () => {
  window.addEventListener('keydown', skip)
  window.addEventListener('pointerdown', skip)
  if (reduced) {
    // Static frame: the mark and the count, then a short fade.
    window.setTimeout(() => (fading.value = true), 900)
    window.setTimeout(() => done(null), 1150)
    return
  }
  const { createSplash } = await import('./scene')
  const css = getComputedStyle(document.documentElement)
  const token = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback
  splash = createSplash(
    canvas.value!,
    {
      streams: Array.from({ length: Math.max(1, total) }, (_, i) => PROJECT_COLOURS[i % PROJECT_COLOURS.length]!),
      restoredAtMs: total === 0 ? [500] : restoredAtMs,
      accent: token('--accent', '#1ec8f0'),
      greys: [token('--logo-bar-low', '#3e4650'), token('--logo-bar-mid', '#4a535e'), token('--logo-bar-high', '#5a646f')],
      background: token('--bg', '#0a0f16'),
      ...variant(),
    },
    (m) => {
      fading.value = true
      done(m)
    },
  )
})

onBeforeUnmount(() => {
  timers.forEach((t) => window.clearTimeout(t))
  window.removeEventListener('keydown', skip)
  window.removeEventListener('pointerdown', skip)
  splash?.dispose()
})
</script>

<template>
  <div class="splash" :class="{ fading }">
    <canvas v-if="!reduced" ref="canvas" class="scene" />
    <div v-else class="static"><ChorusMark :height="96" /></div>
    <div class="caption">
      <div class="word">Chorus</div>
      <div class="count">{{ label }}</div>
    </div>
    <div class="hint">Press any key to skip</div>
  </div>
</template>

<style scoped>
.splash {
  position: fixed;
  inset: 0;
  background: radial-gradient(ellipse at 50% 45%, #10202c 0%, var(--bg) 62%);
  overflow: hidden;
  user-select: none;
  transition: opacity 250ms ease;
}
.splash.fading .caption,
.splash.fading .hint,
.splash.fading .static {
  opacity: 0;
  transition: opacity 250ms ease;
}
.scene {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
}
.static {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  padding-bottom: 60px;
}
.caption {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 54px;
  text-align: center;
  pointer-events: none;
}
.word {
  font-family: var(--font-heading);
  font-weight: 650;
  font-size: 30px;
  letter-spacing: 0.02em;
  color: var(--text);
}
.count {
  margin-top: 6px;
  color: var(--text-muted);
  font-size: 13px;
  font-variant-numeric: tabular-nums;
}
.hint {
  position: absolute;
  right: 16px;
  bottom: 12px;
  color: var(--text-muted);
  opacity: 0.55;
  font-size: 11px;
}
</style>
