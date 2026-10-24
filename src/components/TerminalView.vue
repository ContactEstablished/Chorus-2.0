<script setup lang="ts">
/**
 * One xterm.js terminal bound to a PTY stream (spike S1): the session view's Raw terminal and each
 * tile on the Wall.
 * Output arrives as raw bytes and is handed to xterm unchanged, so its UTF-8 decoder keeps state
 * across chunks. Input goes back as typed text; resizes follow the element's size.
 * With `searchable` (the session view's terminal, not Wall tiles), it can search its buffer (P7),
 * and Ctrl+Shift+F asks the view to open its search bar; agents and shells don't bind that key.
 */
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebglAddon } from '@xterm/addon-webgl'
import { SearchAddon } from '@xterm/addon-search'
import '@xterm/xterm/css/xterm.css'
import { TERMINAL_OPTIONS, themeFromTokens } from './terminal'

const props = defineProps<{ searchable?: boolean }>()
const emit = defineEmits<{
  input: [data: string]
  resize: [cols: number, rows: number]
  search: []
}>()

const host = ref<HTMLDivElement | null>(null)
const renderer = ref<'webgl' | 'dom'>('dom')
let term: Terminal | null = null
let fit: FitAddon | null = null
let search: SearchAddon | null = null
/** The current search's matches: the active one (0-based, -1 past the highlight limit) and the total. */
const results = ref<{ index: number; count: number } | null>(null)
let observer: ResizeObserver | null = null
// Follows the theme: Settings switches it by changing `data-theme` on the root element.
const themeWatch = new MutationObserver(() => {
  if (term) term.options.theme = themeFromTokens()
})

onMounted(() => {
  // Search's match highlights and count use decorations, which xterm 6 still calls proposed API.
  term = new Terminal({ ...TERMINAL_OPTIONS, allowProposedApi: !!props.searchable, theme: themeFromTokens() })
  themeWatch.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
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
  if (props.searchable) {
    search = new SearchAddon()
    term.loadAddon(search)
    search.onDidChangeResults(({ resultIndex, resultCount }) => (results.value = { index: resultIndex, count: resultCount }))
    term.attachCustomKeyEventHandler((e) => {
      if (e.type === 'keydown' && e.ctrlKey && e.shiftKey && !e.altKey && e.key.toLowerCase() === 'f') {
        e.preventDefault()
        emit('search')
        return false
      }
      return true
    })
  }
  // Before xterm's own paste handling (an ancestor's capture listener runs first).
  host.value!.addEventListener('paste', onPaste, { capture: true })
  term.onData((data) => emit('input', data))
  term.onResize(({ cols, rows }) => emit('resize', cols, rows))
  observer = new ResizeObserver(() => fit?.fit())
  observer.observe(host.value!)
})

/**
 * Dictation compatibility (D23): pasted text is never submitted on its own. Dictation apps often
 * paste a sentence with a line break at the end; without bracketed paste (a shell, or a TUI that
 * didn't ask for it) that break would press Enter, so it is dropped. With bracketed paste the
 * agent gets the text as one paste and doesn't submit it, so it passes unchanged.
 */
function onPaste(e: ClipboardEvent): void {
  const text = e.clipboardData?.getData('text/plain')
  if (!term || !text || term.modes.bracketedPasteMode) return
  const kept = text.replace(/[\r\n]+$/, '')
  if (kept === text) return
  e.preventDefault()
  e.stopPropagation()
  if (kept) term.paste(kept)
}

onBeforeUnmount(() => {
  host.value?.removeEventListener('paste', onPaste, { capture: true })
  observer?.disconnect()
  themeWatch.disconnect()
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

// The addon takes #RRGGBB backgrounds only (no alpha).
const MATCH = { matchBackground: '#5c4a12', matchOverviewRuler: '#ffc400', activeMatchBackground: '#136b80', activeMatchColorOverviewRuler: '#1ec8f0' }

/** Finds `text` in the buffer, scrollback included, forwards or backwards from the last match.
 * `typing` (search as you type) keeps the match under the selection when it still matches, rather
 * than moving past it. */
function find(text: string, backwards = false, caseSensitive = false, typing = false): boolean {
  if (!search) return false
  if (!text) {
    clearSearch()
    return false
  }
  const options = { caseSensitive, decorations: MATCH, incremental: typing && !backwards }
  const found = backwards ? search.findPrevious(text, options) : search.findNext(text, options)
  if (!found) results.value = { index: -1, count: 0 }
  return found
}

function clearSearch(): void {
  search?.clearDecorations()
  term?.clearSelection()
  results.value = null
}

defineExpose({ write, size, focus, screenText, bufferText, renderer, find, clearSearch, results })
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
