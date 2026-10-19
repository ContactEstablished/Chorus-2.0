<script setup lang="ts">
/**
 * One xterm.js terminal bound to a PTY stream (spike S1; the Raw view builds on it in P1/P4).
 * Output arrives as raw bytes and is handed to xterm unchanged, so its UTF-8 decoder keeps state
 * across chunks. Input goes back as typed text; resizes follow the element's size.
 */
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebglAddon } from '@xterm/addon-webgl'
import '@xterm/xterm/css/xterm.css'
import { TERMINAL_OPTIONS } from './terminal'

const emit = defineEmits<{
  input: [data: string]
  resize: [cols: number, rows: number]
}>()

const host = ref<HTMLDivElement | null>(null)
const renderer = ref<'webgl' | 'dom'>('dom')
let term: Terminal | null = null
let fit: FitAddon | null = null
let observer: ResizeObserver | null = null

onMounted(() => {
  term = new Terminal({ ...TERMINAL_OPTIONS, theme: { ...TERMINAL_OPTIONS.theme } })
  fit = new FitAddon()
  term.loadAddon(fit)
  term.open(host.value!)
  try {
    const webgl = new WebglAddon()
    // Fall back to the DOM renderer if the GPU context goes away, and say so.
    webgl.onContextLoss(() => {
      webgl.dispose()
      renderer.value = 'dom'
    })
    term.loadAddon(webgl)
    renderer.value = 'webgl'
  } catch {
    renderer.value = 'dom'
  }
  fit.fit()
  term.onData((data) => emit('input', data))
  term.onResize(({ cols, rows }) => emit('resize', cols, rows))
  observer = new ResizeObserver(() => fit?.fit())
  observer.observe(host.value!)
})

onBeforeUnmount(() => {
  observer?.disconnect()
  term?.dispose()
})

/** Writes PTY bytes; `done` runs once xterm has parsed them. */
function write(bytes: Uint8Array, done?: () => void): void {
  term?.write(bytes, done)
}

function size(): { cols: number; rows: number } {
  return { cols: term?.cols ?? 80, rows: term?.rows ?? 24 }
}

function focus(): void {
  term?.focus()
}

/** The visible screen as text, for tests and spike harnesses. */
function screenText(): string {
  if (!term) return ''
  const buf = term.buffer.active
  const lines: string[] = []
  for (let y = 0; y < term.rows; y++) lines.push(buf.getLine(buf.viewportY + y)?.translateToString(true) ?? '')
  return lines.join('\n')
}

/** Every line in the buffer, scrollback included, for tests. */
function bufferText(): string {
  if (!term) return ''
  const buf = term.buffer.active
  const lines: string[] = []
  for (let y = 0; y < buf.length; y++) lines.push(buf.getLine(y)?.translateToString(true) ?? '')
  return lines.join('\n')
}

defineExpose({ write, size, focus, screenText, bufferText, renderer })
</script>

<template>
  <div ref="host" class="terminal" />
</template>

<style scoped>
.terminal {
  width: 100%;
  height: 100%;
  min-height: 0;
  background: var(--bg);
  padding: 6px;
}
</style>
