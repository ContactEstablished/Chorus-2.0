<script setup lang="ts">
/**
 * Spike S1/S4/S5 harness, shown in dev builds only. It drives the PTY host and measures the UI
 * while output streams. `window.__spike` lets a DevTools Protocol script run the same steps.
 * Not product UI: P1's bare Raw view replaces it.
 */
import { onBeforeUnmount, onMounted, ref, shallowRef } from 'vue'
import { Channel } from '@tauri-apps/api/core'
import TerminalView from '../components/TerminalView.vue'
import { commands, events } from '../ipc'
import type { AgentKind, CloseReport, PtyExited, SpawnResult } from '../ipc'

const term = ref<InstanceType<typeof TerminalView> | null>(null)
const session = shallowRef<SpawnResult | null>(null)
const status = ref('idle')
const exits = ref<PtyExited[]>([])
const termBox = ref({ width: '100%', height: '420px' })

let bytesIn = 0
let lastByteAt = 0
let pendingWrites = 0
let messages = 0
let unlistenExit: (() => void) | null = null

onMounted(async () => {
  unlistenExit = await events.ptyExited.listen((e) => {
    exits.value = [...exits.value, e.payload]
    status.value = `session ${e.payload.sessionId} exited with ${e.payload.exitCode}`
  })
})
onBeforeUnmount(() => unlistenExit?.())

async function spawn(
  agent: AgentKind,
  credential: { label: string; envVar: string } | null = null,
  opencodeAttach: { port: number; sessionId: string } | null = null,
  spikeArgs: string[] | null = null,
  spikeEnv: [string, string][] | null = null,
): Promise<SpawnResult> {
  const output = new Channel<ArrayBuffer>()
  output.onmessage = (buf) => {
    const bytes = new Uint8Array(buf)
    bytesIn += bytes.length
    messages++
    lastByteAt = performance.now()
    pendingWrites++
    term.value?.write(bytes, () => pendingWrites--)
  }
  const { cols, rows } = term.value?.size() ?? { cols: 80, rows: 24 }
  const res = await commands.ptySpawn({ agent, cols, rows, credential, opencodeAttach, spikeArgs, spikeEnv }, output)
  if (res.status === 'error') {
    status.value = `spawn failed: ${res.error}`
    throw new Error(res.error)
  }
  session.value = res.data
  status.value = `running ${agent} (pid ${res.data.pid})`
  term.value?.focus()
  return res.data
}

async function write(data: string): Promise<void> {
  if (!session.value) throw new Error('no session')
  const res = await commands.ptyWrite(session.value.sessionId, data)
  if (res.status === 'error') throw new Error(res.error)
}

/** Prompt text and Enter as two separate writes (LESSONS L4). */
async function submit(text: string): Promise<void> {
  await write(text)
  await new Promise((r) => setTimeout(r, 150))
  await write('\r')
}

async function resize(cols: number, rows: number): Promise<void> {
  if (!session.value) return
  const res = await commands.ptyResize(session.value.sessionId, cols, rows)
  if (res.status === 'error') throw new Error(res.error)
}

async function close(): Promise<CloseReport> {
  if (!session.value) throw new Error('no session')
  const res = await commands.ptyClose(session.value.sessionId)
  if (res.status === 'error') throw new Error(res.error)
  status.value = `closed; job had ${res.data.jobPids.length} process(es)`
  session.value = null
  return res.data
}

interface FrameStats {
  bytes: number
  messages: number
  durationMs: number
  frames: number
  maxFrameMs: number
  p95FrameMs: number
  p99FrameMs: number
  over50ms: number
  over100ms: number
  longTasks: number
  longestTaskMs: number
  renderer: string
}

/** Writes `command`, then samples frame times until output has been quiet for `quietMs`. */
async function measure(command: string, quietMs = 750, timeoutMs = 120_000): Promise<FrameStats> {
  const frames: number[] = []
  const tasks: number[] = []
  const obs = new PerformanceObserver((list) => list.getEntries().forEach((e) => tasks.push(e.duration)))
  try {
    obs.observe({ type: 'longtask', buffered: false })
  } catch {
    /* longtask unsupported */
  }
  const startBytes = bytesIn
  const startMessages = messages
  const start = performance.now()
  lastByteAt = start
  let prev = start
  let running = true
  const tick = (t: number) => {
    frames.push(t - prev)
    prev = t
    if (running) requestAnimationFrame(tick)
  }
  requestAnimationFrame(tick)
  await submit(command)
  await new Promise<void>((resolve) => {
    const check = () => {
      const now = performance.now()
      const quiet = now - lastByteAt > quietMs && pendingWrites === 0 && bytesIn > startBytes
      if (quiet || now - start > timeoutMs) resolve()
      else setTimeout(check, 50)
    }
    setTimeout(check, 200)
  })
  running = false
  obs.disconnect()
  const end = lastByteAt
  const sorted = [...frames].sort((a, b) => a - b)
  const pct = (p: number) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))] ?? 0
  return {
    bytes: bytesIn - startBytes,
    messages: messages - startMessages,
    durationMs: Math.round(end - start),
    frames: frames.length,
    maxFrameMs: Math.round(sorted[sorted.length - 1] ?? 0),
    p95FrameMs: Math.round(pct(0.95)),
    p99FrameMs: Math.round(pct(0.99)),
    over50ms: frames.filter((f) => f > 50).length,
    over100ms: frames.filter((f) => f > 100).length,
    longTasks: tasks.length,
    longestTaskMs: Math.round(Math.max(0, ...tasks)),
    renderer: term.value?.renderer ?? 'unknown',
  }
}

function setTermBox(width: string, height: string): void {
  termBox.value = { width, height }
}

async function addFakeKey(label: string) {
  const res = await commands.spikeVaultAddFakeKey(label)
  if (res.status === 'error') throw new Error(res.error)
  return res.data
}

async function importSecretFile(label: string, fileName: string) {
  const res = await commands.spikeVaultImportFile(label, fileName)
  if (res.status === 'error') throw new Error(res.error)
  return { label: res.data.label, len: res.data.keyLen, fingerprint: res.data.fingerprint }
}

async function screenKeyFragment(label: string): Promise<number> {
  const res = await commands.spikeVaultContains(label, term.value?.screenText() ?? '')
  if (res.status === 'error') throw new Error(res.error)
  return res.data
}

async function openFleet() {
  const res = await commands.spikeFleetOpen()
  if (res.status === 'error') throw new Error(res.error)
  return res.data
}

async function probeFleet() {
  const res = await commands.spikeFleetProbe()
  if (res.status === 'error') throw new Error(res.error)
  return res.data
}

const api = {
  openFleet,
  probeFleet,
  spawn,
  addFakeKey,
  importSecretFile,
  screenKeyFragment,
  write,
  submit,
  resize,
  close,
  measure,
  setTermBox,
  screen: () => term.value?.screenText() ?? '',
  size: () => term.value?.size(),
  session: () => session.value,
  exits: () => exits.value,
  bytesIn: () => bytesIn,
}
;(window as unknown as { __spike: typeof api }).__spike = api

const agents: AgentKind[] = ['shell', 'claude', 'codex', 'opencode', 'grok']
</script>

<template>
  <section class="spike">
    <header>
      <strong>Spike harness</strong>
      <span class="muted">dev builds only · {{ status }}</span>
    </header>
    <div class="row">
      <button v-for="a in agents" :key="a" :disabled="!!session" @click="spawn(a)">{{ a }}</button>
      <button :disabled="!session" @click="close()">Close</button>
    </div>
    <p v-if="session" class="muted mono">
      pid {{ session.pid }} · {{ session.program }} {{ session.args.join(' ') }} · env: {{ session.envNames.join(', ') }}
    </p>
    <div class="term" :style="termBox">
      <TerminalView
        ref="term"
        @input="(d) => session && write(d)"
        @resize="(c, r) => resize(c, r)"
      />
    </div>
  </section>
</template>

<style scoped>
.spike {
  margin-top: 24px;
  padding: 16px;
  border: 1px dashed var(--border-strong);
  border-radius: var(--radius-card);
}
header {
  display: flex;
  gap: 12px;
  align-items: baseline;
  margin-bottom: 10px;
}
.row {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  margin-bottom: 8px;
}
button {
  background: var(--surface-chip);
  color: var(--text);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-control);
  padding: 6px 12px;
  font: inherit;
  cursor: pointer;
}
button:disabled {
  opacity: 0.4;
  cursor: default;
}
.muted {
  color: var(--text-muted);
}
.mono {
  font-family: var(--font-mono);
  font-size: 12px;
  overflow-wrap: anywhere;
}
.term {
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
  overflow: hidden;
}
</style>
