<script setup lang="ts">
/**
 * The startup splash window (spike S7, P4). It runs beside session restore and never holds it up:
 * the back end restores sessions whatever the splash is doing, and this window only reflects
 * progress. Any key or click skips it. Reduced-motion users get a short static frame.
 *
 * At start (`?live=1`) the count is real: it asks the back end how many sessions are coming back
 * and how many are back, and each stream takes its session's project colour. The particles keep
 * their planned ~2 s choreography. Without `live`, `?sessions=N` simulates N restores (dev).
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import ChorusMark from '../components/ChorusMark.vue'
import { commands, type RestoreProgress } from '../ipc'
import type { Splash, SplashMetrics } from './scene'

const params = new URLSearchParams(window.location.search)
const live = params.get('live') === '1'
if (params.get('theme') === 'dusk') document.documentElement.dataset.theme = 'dusk'
const reduced = params.get('motion') === 'reduce' || window.matchMedia('(prefers-reduced-motion: reduce)').matches
const PROJECT_COLOURS = ['#1ec8f0', '#f472b6', '#e0a96d', '#a5b4fc', '#c4e05a', '#00e676', '#ffc400', '#4d7cff']
const POLL_MS = 120

const canvas = ref<HTMLCanvasElement | null>(null)
const total = ref(live ? 0 : Math.max(0, Math.min(24, Number(params.get('sessions') ?? 6))))
const restored = ref(0)
const fading = ref(false)
const label = computed(() => {
  const n = total.value
  if (n === 0) return 'Starting'
  const s = n === 1 ? 'session' : 'sessions'
  return restored.value < n ? `Restoring ${restored.value} of ${n} ${s}` : `${n} ${s} restored`
})

/** When each stream arrives, spread over the first ~2 s. */
const schedule = (n: number) => Array.from({ length: n }, (_, i) => 350 + Math.round((1700 * (i + 0.5)) / Math.max(1, n)))
const timers: number[] = []
let poll: number | null = null

/** Live: the real total, colours and count. Simulated: timers. Returns each stream's colour or null. */
async function startProgress(): Promise<(string | null)[]> {
  if (!live) {
    for (const at of schedule(total.value)) timers.push(window.setTimeout(() => restored.value++, at))
    return Array.from({ length: total.value }, (_, i) => PROJECT_COLOURS[i % PROJECT_COLOURS.length]!)
  }
  let first: RestoreProgress
  try {
    first = await commands.restoreProgress()
  } catch {
    return []
  }
  total.value = first.total
  restored.value = first.restored
  poll = window.setInterval(async () => {
    const p = await commands.restoreProgress().catch(() => null)
    if (p) restored.value = p.restored
    if (!p || p.restored >= p.total) window.clearInterval(poll!)
  }, POLL_MS)
  return first.colours
}

let splash: Splash | null = null
const report: { metrics: SplashMetrics | null; reduced: boolean; navToScriptMs: number; timeOrigin: number } = {
  metrics: null,
  reduced,
  navToScriptMs: Math.round(performance.now()),
  timeOrigin: performance.timeOrigin,
}
;(window as unknown as { __splash: typeof report }).__splash = report

function done(metrics: SplashMetrics | null): void {
  report.metrics = metrics
  window.setTimeout(() => void getCurrentWindow().close(), 50)
}

function skip(): void {
  if (splash) splash.skip()
  else if (reduced) done(null)
}

onMounted(async () => {
  window.addEventListener('keydown', skip)
  window.addEventListener('pointerdown', skip)
  const colours = await startProgress()
  if (reduced) {
    // Static frame: the mark and the count, then a short fade.
    window.setTimeout(() => (fading.value = true), 900)
    window.setTimeout(() => done(null), 1150)
    return
  }
  const { createSplash } = await import('./scene')
  const css = getComputedStyle(document.documentElement)
  const token = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback
  const accent = token('--accent', '#1ec8f0')
  splash = createSplash(
    canvas.value!,
    {
      streams: colours.length ? colours.map((c) => c ?? accent) : [accent],
      restoredAtMs: colours.length ? schedule(colours.length) : [500],
      accent,
      greys: [token('--logo-bar-low', '#3e4650'), token('--logo-bar-mid', '#4a535e'), token('--logo-bar-high', '#5a646f')],
      background: token('--bg', '#0a0f16'),
    },
    (m) => {
      fading.value = true
      done(m)
    },
  )
})

onBeforeUnmount(() => {
  timers.forEach((t) => window.clearTimeout(t))
  if (poll !== null) window.clearInterval(poll)
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
