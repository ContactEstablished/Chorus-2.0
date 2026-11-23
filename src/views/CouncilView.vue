<script setup lang="ts">
/**
 * Council (P10, D136–D138). The Docket lists a project's council runs; New council takes a brief
 * and the folder to read (the project's, or another such as a redacted copy, D139), shows the
 * questions it will ask before anything is spent, and runs it. A run shows its phases,
 * each participant's turns with the code they read, the members' agreement per question beside the
 * arbiter's ruling, the findings, and what it cost from both sources.
 */
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import {
  commands,
  events,
  unwrap,
  type CouncilBriefCheck,
  type CouncilCost,
  type CouncilDocket,
  type CouncilPhase,
  type CouncilQuestionState,
  type CouncilRunStatus,
  type CouncilRunView,
  type CouncilTurnDetail,
  type CouncilTurnView,
} from '../ipc'
import { useProjectsStore } from '../stores/projects'
import { useViewStore } from '../stores/view'

const view = useViewStore()
const projects = useProjectsStore()

const projectId = ref<number | null>(null)
const docket = ref<CouncilDocket | null>(null)
const run = ref<CouncilRunView | null>(null)
const openRun = ref<number | null>(null)
const error = ref('')
const busy = ref(false)
const now = ref(Date.now())

/** New council: the chosen brief and what checking it said. */
const choosing = ref(false)
const brief = ref<CouncilBriefCheck | null>(null)
/** The folder the new council reads, when it isn't the project's own. */
const folder = ref<string | null>(null)
/** What the new council would read: the chosen folder, or the project's when it's usable. */
const readFolder = computed(() => folder.value ?? (docket.value?.folderProblem ? null : (docket.value?.folder ?? null)))
const sameFolder = (a: string | null | undefined, b: string | null | undefined) =>
  !!a && !!b && a.replace(/[\\/]+$/, '').toLowerCase() === b.replace(/[\\/]+$/, '').toLowerCase()

/** The turn whose answer, prompt and tool calls are open. */
const openTurn = ref<number | null>(null)
const turnDetail = ref<CouncilTurnDetail | null>(null)
const showPrompt = ref(false)
const confirmRemove = ref<number | null>(null)

const choices = computed(() => projects.usable)

async function loadDocket(): Promise<void> {
  if (projectId.value === null) {
    docket.value = null
    return
  }
  try {
    docket.value = await unwrap(commands.councilDocket(projectId.value))
  } catch (e) {
    error.value = String(e)
  }
}

async function loadRun(): Promise<void> {
  if (openRun.value === null) {
    run.value = null
    return
  }
  try {
    run.value = await unwrap(commands.councilRun(openRun.value))
  } catch (e) {
    error.value = String(e)
  }
}

let unlisten: (() => void) | null = null
let timer: ReturnType<typeof setInterval> | null = null
onMounted(async () => {
  await projects.load()
  const preferred = view.projectFilter ?? choices.value.find((p) => p.folder)?.id ?? choices.value[0]?.id ?? null
  projectId.value = preferred
  unlisten = await events.councilUpdated.listen((e) => {
    void loadDocket()
    if (e.payload.runId === openRun.value) void loadRun()
  })
  timer = setInterval(() => (now.value = Date.now()), 1000)
})
onUnmounted(() => {
  unlisten?.()
  if (timer) clearInterval(timer)
})

watch(projectId, () => {
  openRun.value = null
  run.value = null
  choosing.value = false
  brief.value = null
  folder.value = null
  error.value = ''
  void loadDocket()
})

function open(id: number): void {
  openRun.value = id
  openTurn.value = null
  turnDetail.value = null
  choosing.value = false
  error.value = ''
  void loadRun()
}

/** Opens the running council, switching to its project first if it's another one. */
async function showRunning(): Promise<void> {
  const id = docket.value?.running
  if (id === null || id === undefined) return
  try {
    const r = await unwrap(commands.councilRun(id))
    if (r.projectId !== projectId.value) {
      projectId.value = r.projectId
      await nextTick()
    }
    open(id)
  } catch (e) {
    error.value = String(e)
  }
}

function back(): void {
  openRun.value = null
  run.value = null
  error.value = ''
  void loadDocket()
}

function startNew(): void {
  choosing.value = true
  brief.value = null
  folder.value = null
  openRun.value = null
  run.value = null
  error.value = ''
}

async function pickBrief(): Promise<void> {
  error.value = ''
  const path = await commands.councilBriefPick(readFolder.value ?? docket.value?.folder ?? null)
  if (!path) return
  try {
    brief.value = await unwrap(commands.councilBriefCheck(path))
  } catch (e) {
    error.value = String(e)
    brief.value = null
  }
}

async function pickFolder(): Promise<void> {
  error.value = ''
  const path = await commands.councilFolderPick(folder.value ?? docket.value?.folder ?? null)
  if (!path) return
  folder.value = sameFolder(path, docket.value?.folder) && !docket.value?.folderProblem ? null : path
}

async function start(): Promise<void> {
  if (!brief.value || projectId.value === null || !readFolder.value) return
  busy.value = true
  error.value = ''
  try {
    const id = await unwrap(commands.councilStart(projectId.value, brief.value.path, folder.value))
    choosing.value = false
    brief.value = null
    folder.value = null
    open(id)
    void loadDocket()
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
  }
}

async function cancel(): Promise<void> {
  if (!run.value) return
  try {
    await unwrap(commands.councilCancel(run.value.id))
  } catch (e) {
    error.value = String(e)
  }
}

async function remove(id: number): Promise<void> {
  try {
    await unwrap(commands.councilRunDelete(id))
    confirmRemove.value = null
    if (openRun.value === id) back()
    else void loadDocket()
  } catch (e) {
    error.value = String(e)
  }
}

async function openFindings(): Promise<void> {
  if (!run.value) return
  try {
    await unwrap(commands.councilFindingsOpen(run.value.id))
  } catch (e) {
    error.value = String(e)
  }
}

async function toggleTurn(t: CouncilTurnView): Promise<void> {
  if (openTurn.value === t.turn.id) {
    openTurn.value = null
    turnDetail.value = null
    return
  }
  openTurn.value = t.turn.id
  showPrompt.value = false
  turnDetail.value = null
  try {
    turnDetail.value = await unwrap(commands.councilTurn(t.turn.id))
  } catch (e) {
    error.value = String(e)
  }
}

const PHASES: { key: CouncilPhase; label: string }[] = [
  { key: 'positions', label: 'Positions (blind)' },
  { key: 'critique', label: 'Critique (anonymous)' },
  { key: 'arbitration', label: 'Arbitration' },
  { key: 'synthesis', label: 'Synthesis' },
  { key: 'done', label: 'Done' },
]

function phaseState(key: CouncilPhase): 'done' | 'active' | 'halted' | 'pending' {
  const r = run.value
  if (!r) return 'pending'
  const order = PHASES.map((p) => p.key)
  const at = order.indexOf(r.phase)
  const here = order.indexOf(key)
  if (r.status === 'complete') return 'done'
  if (here < at) return 'done'
  if (here === at) return r.status === 'running' ? 'active' : 'halted'
  return 'pending'
}

const turnsByPhase = computed(() => {
  const groups = new Map<CouncilPhase, CouncilTurnView[]>()
  for (const t of run.value?.turns ?? []) {
    const list = groups.get(t.turn.phase) ?? []
    list.push(t)
    groups.set(t.turn.phase, list)
  }
  return PHASES.filter((p) => groups.has(p.key)).map((p) => ({ ...p, turns: groups.get(p.key) ?? [] }))
})

const STATE_LABEL: Record<CouncilQuestionState, string> = {
  agreed: 'agreed',
  qualified: 'qualified',
  split: 'split',
  disagreed: 'disagreed',
  'not-measured': 'not measured',
}

const STATUS_LABEL: Record<CouncilRunStatus, string> = {
  running: 'running',
  complete: 'complete',
  failed: 'failed',
  cancelled: 'cancelled',
  abandoned: 'abandoned',
}

function ago(ms: number | null): string {
  if (ms === null) return ''
  const s = Math.max(0, Math.round((now.value - ms) / 1000))
  if (s < 60) return 'just now'
  if (s < 3600) return `${Math.round(s / 60)} min ago`
  if (s < 86400) return `${Math.round(s / 3600)} h ago`
  return new Date(ms).toLocaleDateString()
}

function duration(from: number | null, to: number | null): string {
  if (from === null) return ''
  const s = Math.max(0, Math.round(((to ?? now.value) - from) / 1000))
  if (s < 60) return `${s} s`
  if (s < 3600) return `${Math.floor(s / 60)} min ${s % 60} s`
  return `${Math.floor(s / 3600)} h ${Math.round((s % 3600) / 60)} min`
}

function money(n: number | null): string {
  if (n === null) return '—'
  return n < 0.01 ? `$${n.toFixed(4)}` : `$${n.toFixed(2)}`
}

function costLine(c: CouncilCost): string {
  const parts: string[] = []
  if (c.reportedUsd !== null) parts.push(`${money(c.reportedUsd)} reported`)
  if (c.keyUsd !== null) parts.push(`${money(c.keyUsd)} by the key`)
  if (c.subscriptionTurns > 0) parts.push(`${c.subscriptionTurns} turn${c.subscriptionTurns === 1 ? '' : 's'} on subscriptions`)
  return parts.join(' · ') || 'cost not known yet'
}

function kb(n: number): string {
  return n < 1000 ? `${n} B` : n < 1_000_000 ? `${(n / 1000).toFixed(1)} KB` : `${(n / 1_000_000).toFixed(2)} MB`
}

function outcomeLabel(t: CouncilTurnView): string {
  if (t.turn.outcome === null) return t.doing ? `reading · ${t.doing}` : 'thinking…'
  return t.turn.outcome
}
</script>

<template>
  <section class="page">
    <header class="head">
      <div>
        <h1>Council</h1>
        <p class="lead">
          A panel of models reviews a brief. Members answer its numbered questions blind, critique each other anonymously, and an arbiter rules on each
          question and writes the findings beside the brief. Every participant can read this project's code, read-only, to check the brief against it.
        </p>
      </div>
      <div class="head-controls">
        <label class="project">
          Project
          <select v-model="projectId">
            <option v-for="p in choices" :key="p.id" :value="p.id">{{ p.name }}</option>
          </select>
        </label>
        <button
          type="button"
          class="primary"
          :disabled="!docket || !!docket.reason || docket.running !== null"
          :title="docket?.reason ?? (docket?.running !== null ? 'A council is already running' : '')"
          @click="startNew"
        >
          New council
        </button>
      </div>
    </header>

    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-if="!choices.length" class="hint">Add a project with a folder in Settings → Projects first: a council reads the project's code.</p>
    <p v-else-if="docket?.reason" class="hint warn">
      {{ docket.reason }}
      <button type="button" class="link" @click="view.openSettings('council')">Settings → Council</button>
    </p>
    <p v-if="docket && docket.running !== null && run?.id !== docket.running" class="hint">
      A council is running, so a new one waits until it finishes.
      <button type="button" class="link" @click="showRunning">Show it</button>
    </p>

    <!-- New council -->
    <div v-if="choosing" class="panel">
      <h2>New council</h2>
      <p class="muted">Choose a Markdown brief with numbered questions, and the folder the council reads. Nothing is sent until you press Run.</p>
      <div class="choice">
        <span class="choice-label">Brief</span>
        <div class="row">
          <button type="button" @click="pickBrief">Choose brief…</button>
          <span v-if="brief" class="mono">{{ brief.path }}</span>
        </div>
      </div>
      <div class="choice">
        <span class="choice-label">Folder to read</span>
        <div class="row">
          <button type="button" @click="pickFolder">Choose folder…</button>
          <span v-if="readFolder" class="mono">{{ readFolder }}</span>
          <span v-if="!folder && readFolder" class="muted small">(the project’s folder)</span>
          <button v-if="folder && docket?.folder && !docket.folderProblem" type="button" class="link" @click="folder = null">Use the project’s folder</button>
        </div>
        <p v-if="!readFolder && docket?.folderProblem" class="error">{{ docket.folderProblem }} Choose a folder to read instead.</p>
        <p class="muted small">
          Every member can read every file in this folder, read-only. To keep passwords away from the models, choose a redacted copy of the project.
          The run is still listed under {{ docket?.projectName ?? 'this project' }}.
        </p>
      </div>
      <template v-if="brief">
        <p v-if="brief.problem" class="error">{{ brief.problem }}</p>
        <template v-else>
          <p class="muted">The council will be asked {{ brief.questions.length }} question{{ brief.questions.length === 1 ? '' : 's' }}:</p>
          <ol class="questions">
            <li v-for="(q, i) in brief.questions" :key="i">{{ q }}</li>
          </ol>
          <p class="muted">Findings will be written to <span class="mono">{{ brief.findingsPath }}</span> (or -2, -3… if that exists).</p>
        </template>
      </template>
      <div class="row">
        <button type="button" class="primary" :disabled="busy || !brief || !!brief.problem || !readFolder" @click="start">Run council</button>
        <button type="button" :disabled="busy" @click="choosing = false">Cancel</button>
      </div>
    </div>

    <!-- A run -->
    <div v-else-if="run" class="run">
      <div class="run-head">
        <div>
          <button type="button" class="link" :disabled="run.status === 'running'" @click="back">← Docket</button>
          <h2>{{ run.briefName }}</h2>
          <p class="muted">
            <span class="status" :class="run.status">{{ STATUS_LABEL[run.status] }}</span>
            · started {{ ago(run.startedAt) }} · {{ duration(run.startedAt, run.endedAt) }} · {{ run.roster.length }} participants
          </p>
          <p class="muted small">Read <span class="mono">{{ run.folder }}</span></p>
        </div>
        <div class="row">
          <button v-if="run.status === 'running'" type="button" class="danger" @click="cancel">Cancel run</button>
          <button v-if="run.findingsPath" type="button" @click="openFindings">Open findings</button>
          <template v-if="run.status !== 'running'">
            <button v-if="confirmRemove !== run.id" type="button" @click="confirmRemove = run.id">Remove</button>
            <template v-else>
              <button type="button" class="danger" @click="remove(run.id)">Remove from Docket</button>
              <button type="button" @click="confirmRemove = null">Keep</button>
            </template>
          </template>
        </div>
      </div>
      <p v-if="confirmRemove === run.id" class="muted">Removes this run and its transcript from Chorus. The findings file beside the brief is left where it is.</p>
      <p v-if="run.reason && run.status !== 'complete'" class="hint" :class="run.status === 'cancelled' ? '' : 'warn'">{{ run.reason }}</p>

      <ol class="track">
        <li v-for="p in PHASES" :key="p.key" :class="phaseState(p.key)">
          <span class="bar"></span>
          {{ p.label }}
          <small v-if="phaseState(p.key) === 'halted'">stopped</small>
        </li>
      </ol>

      <div v-if="run.verdicts.length" class="panel">
        <h3>Questions</h3>
        <p class="muted small">
          <strong>Members</strong> counts the members' own verdict tokens from the blind round: agreement, not correctness.
          <strong>Arbiter</strong> is the ruling after reading everything{{ run.arbiterAsked ? '' : ' (not given yet)' }}. They are shown apart on purpose.
        </p>
        <table class="verdicts">
          <thead>
            <tr><th></th><th>Question</th><th>Members</th><th>Arbiter</th></tr>
          </thead>
          <tbody>
            <tr v-for="v in run.verdicts" :key="v.index">
              <td class="q">Q{{ v.index + 1 }}</td>
              <td>{{ v.question }}</td>
              <td>
                <span v-if="v.consensus" class="tag" :class="v.consensus">{{ STATE_LABEL[v.consensus] }}</span>
                <span v-else class="muted">—</span>
              </td>
              <td>
                <span v-if="v.ruling" class="tag ruling" :class="v.ruling">{{ v.ruling.toLowerCase().replace(/-/g, ' ') }}</span>
                <span v-else-if="run.arbiterAsked" class="muted">unparsed</span>
                <span v-else class="muted">—</span>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <div v-for="g in turnsByPhase" :key="g.key" class="panel">
        <h3>{{ g.label }}</h3>
        <ul class="turns">
          <li v-for="t in g.turns" :key="t.turn.id" :class="[t.turn.outcome ?? 'live', { open: openTurn === t.turn.id }]">
            <button type="button" class="turn-head" @click="toggleTurn(t)">
              <strong>{{ t.label }}</strong>
              <span class="mono small">{{ t.model }}</span>
              <span class="outcome">{{ outcomeLabel(t) }}</span>
              <span class="small muted">
                {{ t.turn.toolCalls }} tool call{{ t.turn.toolCalls === 1 ? '' : 's' }} · {{ kb(t.turn.toolBytes) }} read
                <template v-if="t.turn.tokensIn !== null"> · {{ t.turn.tokensIn.toLocaleString() }} in / {{ (t.turn.tokensOut ?? 0).toLocaleString() }} out</template>
                <template v-if="t.turn.costUsd !== null"> · {{ money(t.turn.costUsd) }}</template>
                · {{ duration(t.turn.startedAt, t.turn.endedAt) }}
              </span>
            </button>
            <div v-if="openTurn === t.turn.id" class="turn-body">
              <pre v-if="t.turn.content" class="answer">{{ t.turn.content }}</pre>
              <p v-else class="muted">No answer yet.</p>
              <template v-if="turnDetail">
                <details v-if="turnDetail.toolCalls.length">
                  <summary>Code it read ({{ turnDetail.toolCalls.length }})</summary>
                  <ul class="calls">
                    <li v-for="(c, i) in turnDetail.toolCalls" :key="i" :class="{ refused: !c.ok }">
                      <span class="mono">{{ c.call }}</span>
                      <span class="muted small">{{ c.ok ? kb(c.bytes) : c.note }}</span>
                    </li>
                  </ul>
                </details>
                <details @toggle="showPrompt = ($event.target as HTMLDetailsElement).open">
                  <summary>Prompt sent</summary>
                  <pre v-if="showPrompt" class="prompt">{{ turnDetail.prompt }}</pre>
                </details>
              </template>
            </div>
          </li>
        </ul>
      </div>

      <div v-if="run.findingsText || run.findingsProblem" class="panel">
        <h3>Findings</h3>
        <p v-if="run.findingsPath" class="muted small">Written to <span class="mono">{{ run.findingsPath }}</span></p>
        <p v-if="run.findingsProblem" class="hint warn">{{ run.findingsProblem }}</p>
        <pre v-if="run.findingsText" class="findings">{{ run.findingsText }}</pre>
      </div>

      <div class="panel cost">
        <h3>Cost</h3>
        <p>
          <strong>{{ money(run.cost.reportedUsd) }}</strong> reported by OpenRouter with each response
          <span class="muted">({{ run.cost.turnsReporting }} of {{ run.cost.turns }} finished turns reported a cost)</span>
        </p>
        <p>
          <strong>{{ money(run.cost.keyUsd) }}</strong> spent by this run's key, from OpenRouter's analytics after it was revoked
          <span v-if="run.cost.keyNote" class="muted">— {{ run.cost.keyNote }}</span>
        </p>
        <p class="muted small">
          Spend cap {{ money(run.cost.capUsd) }}.
          <template v-if="run.cost.tokensIn !== null">{{ run.cost.tokensIn.toLocaleString() }} tokens in, {{ (run.cost.tokensOut ?? 0).toLocaleString() }} out.</template>
          The two figures come from different places and are not merged; analytics can still grow for a few minutes.
        </p>
      </div>
    </div>

    <!-- The Docket -->
    <div v-else-if="docket" class="docket">
      <h2>Docket <span class="muted small">{{ docket.runs.length }} run{{ docket.runs.length === 1 ? '' : 's' }} for {{ docket.projectName }}</span></h2>
      <p v-if="!docket.runs.length" class="muted">No councils yet for this project. Press New council and choose a brief.</p>
      <ul class="rows">
        <li v-for="r in docket.runs" :key="r.id">
          <button type="button" class="docket-row" @click="open(r.id)">
            <span class="status" :class="r.status">{{ STATUS_LABEL[r.status] }}</span>
            <strong :title="r.briefPath">{{ r.briefName }}</strong>
            <span class="muted small">{{ ago(r.startedAt) }} · {{ duration(r.startedAt, r.endedAt) }} · {{ r.turns }} turns · {{ r.toolCalls }} tool calls · {{ costLine(r.cost) }}</span>
            <span v-if="!sameFolder(r.folder, docket.folder)" class="small muted" :title="r.folder">Read {{ r.folder }}</span>
            <span v-if="r.digest" class="small">{{ r.digest }}</span>
            <span v-else-if="r.reason" class="small muted">{{ r.reason }}</span>
          </button>
        </li>
      </ul>
    </div>
  </section>
</template>

<style scoped>
.page {
  padding: 28px 40px;
  max-width: 1100px;
}
.head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 20px;
}
.head-controls {
  display: flex;
  gap: 10px;
  align-items: flex-end;
}
.head-controls button {
  white-space: nowrap;
}
.project {
  display: grid;
  gap: 4px;
  font-size: 12.5px;
  color: var(--text-2);
}
.project select {
  padding: 7px 10px;
  min-width: 180px;
}
h1 {
  margin: 0 0 6px;
  font-size: 28px;
}
h2 {
  margin: 0 0 8px;
  font-size: 18px;
}
h3 {
  margin: 0 0 10px;
  font-size: 14px;
}
.lead {
  margin: 0 0 22px;
  color: var(--text-2);
  max-width: 660px;
  line-height: 1.45;
}
p {
  font-size: 13px;
  line-height: 1.55;
}
.muted {
  color: var(--text-muted);
}
.small {
  font-size: 12px;
}
.error {
  color: var(--status-failed);
}
.hint {
  padding: 10px 12px;
  border-radius: var(--radius-card);
  background: var(--surface-raised);
}
.hint.warn {
  color: var(--status-needs);
}
.mono {
  font-family: var(--font-mono);
  font-size: 12px;
  word-break: break-all;
}
.choice {
  display: grid;
  gap: 6px;
}
.choice-label {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--text-2);
}
.row {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
button {
  padding: 7px 12px;
}
button.link {
  border: none;
  background: none;
  color: var(--accent);
  padding: 0;
  cursor: pointer;
}
.danger {
  color: var(--status-failed);
}
.panel {
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  background: var(--surface-raised);
  padding: 16px;
  margin: 14px 0;
  display: grid;
  gap: 8px;
}
.questions {
  margin: 0;
  padding-left: 22px;
  font-size: 13px;
  line-height: 1.6;
}
.run-head {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  align-items: flex-start;
}
.status {
  font-size: 11.5px;
  padding: 2px 8px;
  border-radius: var(--radius-pill);
  background: var(--surface-chip);
}
.status.running {
  color: var(--status-working);
}
.status.complete {
  color: var(--status-done);
}
.status.failed,
.status.abandoned {
  color: var(--status-failed);
}
.track {
  list-style: none;
  padding: 0;
  margin: 16px 0;
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: 6px;
  font-size: 12px;
  color: var(--text-muted);
}
.track li {
  display: grid;
  gap: 6px;
}
.track .bar {
  height: 4px;
  border-radius: 2px;
  background: var(--border);
}
.track .done {
  color: var(--text);
}
.track .done .bar {
  background: var(--status-done);
}
.track .active {
  color: var(--text);
}
.track .active .bar {
  background: linear-gradient(90deg, var(--status-working) 0%, var(--border) 50%, var(--status-working) 100%);
  background-size: 200% 100%;
  animation: slide 1.2s linear infinite;
}
.track .halted .bar {
  background: var(--status-failed);
}
@keyframes slide {
  from {
    background-position: 200% 0;
  }
  to {
    background-position: 0 0;
  }
}
@media (prefers-reduced-motion: reduce) {
  .track .active .bar {
    animation: none;
    background: var(--status-working);
  }
}
.verdicts {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}
.verdicts th {
  text-align: left;
  font-weight: 500;
  color: var(--text-muted);
  font-size: 12px;
  padding: 4px 8px;
}
.verdicts td {
  padding: 6px 8px;
  border-top: 1px solid var(--border);
  vertical-align: top;
}
.verdicts .q {
  font-family: var(--font-mono);
  color: var(--text-muted);
}
.tag {
  font-size: 11.5px;
  padding: 2px 8px;
  border-radius: var(--radius-pill);
  background: var(--surface-chip);
  white-space: nowrap;
}
.tag.agreed,
.tag.APPROVED {
  color: var(--status-done);
}
.tag.qualified,
.tag.APPROVED-WITH-REVISIONS {
  color: var(--status-ready);
}
.tag.split,
.tag.REVISE,
.tag.INSUFFICIENT-INFORMATION {
  color: var(--status-needs);
}
.tag.disagreed,
.tag.REJECTED {
  color: var(--status-failed);
}
.turns {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  gap: 6px;
}
.turns li {
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
  background: var(--surface);
}
.turns li.refused {
  border-color: var(--status-failed);
}
.turns li.live {
  border-color: var(--status-working);
}
.turn-head {
  width: 100%;
  display: grid;
  grid-template-columns: 140px 1fr;
  gap: 4px 12px;
  text-align: left;
  border: none;
  background: none;
  cursor: pointer;
  align-items: baseline;
}
.turn-head .small {
  grid-column: 1 / -1;
}
.outcome {
  grid-column: 1 / -1;
  font-size: 12px;
  color: var(--text-2);
  word-break: break-all;
}
.turns li.live .outcome {
  color: var(--status-working);
}
.turn-body {
  padding: 0 12px 12px;
  display: grid;
  gap: 8px;
}
pre {
  white-space: pre-wrap;
  word-break: break-word;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.5;
  margin: 0;
  padding: 10px;
  border-radius: var(--radius-control);
  background: var(--bg);
  max-height: 460px;
  overflow: auto;
}
.findings {
  max-height: 700px;
}
.calls {
  list-style: none;
  padding: 0;
  margin: 6px 0 0;
  display: grid;
  gap: 2px;
  max-height: 260px;
  overflow: auto;
}
.calls li {
  display: flex;
  justify-content: space-between;
  gap: 12px;
}
.calls li.refused .mono {
  color: var(--status-failed);
}
summary {
  cursor: pointer;
  font-size: 12.5px;
}
.rows {
  list-style: none;
  padding: 0;
  margin: 0;
  display: grid;
  gap: 6px;
}
.docket-row {
  width: 100%;
  display: grid;
  grid-template-columns: 90px 1fr;
  gap: 4px 12px;
  text-align: left;
  padding: 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  background: var(--surface-raised);
  cursor: pointer;
}
.docket-row .status {
  justify-self: start;
}
.docket-row .small {
  grid-column: 2;
}
</style>
