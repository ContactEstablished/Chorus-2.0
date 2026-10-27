<script setup lang="ts">
/**
 * The Loop (P15): runs that work a repository's roadmap phase after phase on their own branch.
 * A run lists its phases, each phase its tasks and steps: who did each step, the model asked for
 * and the model that answered, and what the checks said. A run that stops says why and where to
 * look; one at a milestone waits for Approve. Questions a step asked wait here for an answer:
 * blocking ones with no timeout, timed ones until their default is taken (slice 3).
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import {
  commands,
  events,
  unwrap,
  type LoopCheckRun,
  type LoopFolder,
  type LoopQuestion,
  type LoopRun,
  type LoopRunConfig,
  type LoopStep,
  type Flavor,
} from '../ipc'
import { useBotsStore } from '../stores/bots'
import { useViewStore } from '../stores/view'
import { useNoticesStore } from '../stores/notices'
import { parseChecks } from './loopChecks'

const bots = useBotsStore()
const view = useViewStore()
const notices = useNoticesStore()

const runs = ref<LoopRun[]>([])
const open = ref<number | null>(null)
const flavor = ref<Flavor>('release')
const busy = ref(false)
const confirmingDelete = ref<number | null>(null)
const shownOutput = ref<string | null>(null)
/** Answers being written, per question. */
const answers = ref<Record<number, { text: string; rule: boolean }>>({})
const now = ref(Date.now())

interface Draft {
  /** The run being edited, or null for a new one. */
  id: number | null
  name: string
  folder: string
  baseBranch: string
  config: LoopRunConfig
}
const editing = ref<Draft | null>(null)
const folder = ref<LoopFolder | null>(null)
const error = ref<string | null>(null)
/** Text pasted to fill in the checks, or null while that panel is closed. */
const pasting = ref<string | null>(null)
const pasted = computed(() => parseChecks(pasting.value ?? ''))

let timer: ReturnType<typeof setInterval> | null = null
let unlisten: (() => void) | null = null
let failing = false

async function load(): Promise<void> {
  try {
    runs.value = await unwrap(commands.loopRunsList())
    failing = false
  } catch (e) {
    if (!failing) notices.error("Couldn't load the loop's runs.", e)
    failing = true
  }
}

onMounted(async () => {
  void load()
  void bots.load()
  try {
    flavor.value = (await commands.appInfo()).flavor
  } catch {
    /* release: no scripted steps */
  }
  unlisten = await events.loopUpdated.listen(() => void load())
  timer = setInterval(() => {
    now.value = Date.now()
    void load()
  }, 5_000)
})
onUnmounted(() => {
  if (timer) clearInterval(timer)
  unlisten?.()
})

const usableBots = computed(() => bots.list.filter((b) => !b.archived))
const claudeBots = computed(() => usableBots.value.filter((b) => b.agent === 'claude'))

function botId(config: LoopRunConfig, role: 'planner' | 'builder'): number {
  return config.roles.find((r) => r.role === role)?.bot ?? 0
}

function setBot(config: LoopRunConfig, role: 'planner' | 'builder', bot: number): void {
  const rest = config.roles.filter((r) => r.role !== role)
  config.roles = [...rest, { role, bot, fallback: null }]
}

function startNew(): void {
  const first = claudeBots.value[0]?.id ?? usableBots.value[0]?.id ?? 0
  error.value = null
  folder.value = null
  pasting.value = null
  editing.value = {
    id: null,
    name: '',
    folder: '',
    baseBranch: '',
    config: {
      roles: [
        { role: 'planner', bot: first, fallback: null },
        { role: 'builder', bot: first, fallback: null },
      ],
      checks: [{ name: 'tests', command: 'npm test', timeoutSecs: 600 }],
      limits: { fixAttempts: 3, sameSignature: 3, planRetries: 1, stepRetries: 1, phaseMinutes: 240 },
      milestoneEvery: 3,
      script: null,
      phoneStatusOnly: false,
    },
  }
}

function editRun(r: LoopRun): void {
  error.value = null
  folder.value = null
  pasting.value = null
  editing.value = { id: r.id, name: r.name, folder: r.repoRoot, baseBranch: r.baseBranch, config: structuredClone(r.config) }
}

async function checkFolder(): Promise<void> {
  const d = editing.value
  if (!d || !d.folder.trim()) return
  try {
    folder.value = await commands.loopFolderCheck(d.folder.trim(), d.baseBranch.trim() || null)
    if (!d.baseBranch.trim() && folder.value.branch) d.baseBranch = folder.value.branch
  } catch (e) {
    folder.value = { repoRoot: null, branch: null, phases: [], problem: String(e) }
  }
}

function addCheck(): void {
  editing.value?.config.checks.push({ name: '', command: '', timeoutSecs: 600 })
}

function removeCheck(i: number): void {
  editing.value?.config.checks.splice(i, 1)
}

/** The pasted checks replace the list; nothing is kept until the run is saved. */
function usePasted(): void {
  const d = editing.value
  if (!d || !pasted.value.checks.length) return
  d.config.checks = pasted.value.checks.map((c) => ({ ...c }))
  pasting.value = null
}

function plural(n: number, one: string): string {
  return `${n} ${one}${n === 1 ? '' : 's'}`
}

async function save(): Promise<void> {
  const d = editing.value
  if (!d) return
  busy.value = true
  error.value = null
  try {
    const config = { ...d.config, script: d.config.script?.trim() ? d.config.script.trim() : null }
    if (d.id === null) {
      const made = await unwrap(commands.loopRunCreate({ name: d.name, folder: d.folder, baseBranch: d.baseBranch.trim() || null, config }))
      open.value = made.id
    } else {
      await unwrap(commands.loopRunUpdate(d.id, config))
    }
    editing.value = null
    await load()
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = false
  }
}

async function act(r: LoopRun, what: 'start' | 'pause' | 'stop' | 'approve' | 'delete'): Promise<void> {
  busy.value = true
  try {
    if (what === 'start') await unwrap(commands.loopRunStart(r.id))
    else if (what === 'pause') await unwrap(commands.loopRunPause(r.id))
    else if (what === 'stop') await unwrap(commands.loopRunStop(r.id))
    else if (what === 'approve') await unwrap(commands.loopRunApprove(r.id))
    else {
      await unwrap(commands.loopRunDelete(r.id))
      confirmingDelete.value = null
      if (open.value === r.id) open.value = null
    }
    await load()
  } catch (e) {
    notices.error(`Couldn't ${what} ${r.name}.`, e)
  } finally {
    busy.value = false
  }
}

const STATE_TEXT: Record<LoopRun['state'], string> = {
  draft: 'Not started',
  running: 'Running',
  paused: 'Paused',
  waiting: 'Waiting for your review',
  stopped: 'Stopped',
  done: 'Done',
}

function stepLabel(s: LoopStep): string {
  if (s.kind === 'plan') return s.attempt ? `Plan (retry ${s.attempt})` : 'Plan'
  if (s.kind === 'tag') return 'Tag'
  const after = s.attempt ? `fix ${s.attempt}` : 'build'
  if (s.kind === 'check') return `Checks after ${s.task} ${after}`
  return s.attempt ? `Fix ${s.task} (attempt ${s.attempt})` : `Build ${s.task}`
}

function model(s: LoopStep): string {
  if (!s.modelAsked) return ''
  if (!s.modelObserved || s.modelObserved === s.modelAsked) return s.modelAsked
  return `${s.modelAsked} → ${s.modelObserved}`
}

function seconds(ms: number): string {
  return `${(ms / 1000).toFixed(1)} s`
}

function at(secs: number | null): string {
  if (!secs) return ''
  const t = new Date(secs * 1000)
  const sameDay = t.toDateString() === new Date().toDateString()
  return sameDay ? t.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : t.toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
}

function checkKey(s: LoopStep, c: LoopCheckRun): string {
  return `${s.id}:${c.name}`
}

function draft(q: LoopQuestion): { text: string; rule: boolean } {
  if (!answers.value[q.id]) answers.value[q.id] = { text: '', rule: false }
  return answers.value[q.id]!
}

function where(q: LoopQuestion): string {
  return `P${String(q.phase).padStart(2, '0')}${q.task ? ' ' + q.task : ' plan'}`
}

function timeLeft(q: LoopQuestion): string {
  if (!q.deadlineAtSecs) return ''
  const mins = Math.max(0, Math.ceil((q.deadlineAtSecs * 1000 - now.value) / 60_000))
  return mins <= 1 ? 'default in under a minute' : `default in ${mins} min`
}

function outcome(q: LoopQuestion): string {
  if (q.state === 'answered') return `You answered: ${q.answer}${q.madeRule ? ' (made a rule)' : ''}`
  if (q.state === 'defaulted') return `Nobody answered in time; the default was taken: ${q.answer}`
  return 'Closed without an answer'
}

async function answer(q: LoopQuestion): Promise<void> {
  const d = draft(q)
  busy.value = true
  try {
    await unwrap(commands.loopQuestionAnswer(q.id, d.text, d.rule))
    delete answers.value[q.id]
    await load()
  } catch (e) {
    notices.error("Couldn't answer the question.", e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <section class="page">
    <header class="head">
      <div>
        <h1>Loop</h1>
        <p class="lead">
          Runs a repository's roadmap phase after phase, on a branch of its own, until a milestone or a real problem stops it. Each phase is planned,
          built task by task until your checks pass, and tagged. Your main branch is never touched.
        </p>
      </div>
      <button type="button" class="primary" :disabled="!usableBots.length" :title="usableBots.length ? '' : 'Make a Bot first'" @click="startNew">New run</button>
    </header>

    <form v-if="editing" class="editor" @submit.prevent="save">
      <div class="grid">
        <label>Name <input v-model="editing.name" maxlength="40" autocomplete="off" :disabled="editing.id !== null" /></label>
        <label>
          Repository folder
          <input v-model="editing.folder" class="mono" spellcheck="false" autocomplete="off" placeholder="C:\Projects\app" :disabled="editing.id !== null" @change="checkFolder" />
        </label>
        <label>
          Start from branch
          <input v-model="editing.baseBranch" class="mono" spellcheck="false" autocomplete="off" placeholder="its checked-out branch" :disabled="editing.id !== null" @change="checkFolder" />
        </label>
      </div>
      <p v-if="folder?.problem" class="hint warn">{{ folder.problem }}</p>
      <p v-else-if="folder?.phases.length" class="hint">
        The roadmap on {{ folder.branch }} has {{ folder.phases.length }} phase{{ folder.phases.length === 1 ? '' : 's' }}:
        {{ folder.phases.map((p) => `P${String(p.number).padStart(2, '0')} ${p.title}`).join(', ') }}.
      </p>
      <p v-else-if="editing.id === null" class="hint">The branch needs a roadmap at .chorus/run/roadmap.md, with phases headed like "## P01 · Project skeleton".</p>

      <div class="grid">
        <label>
          Phase planner
          <select :value="botId(editing.config, 'planner')" @change="setBot(editing.config, 'planner', Number(($event.target as HTMLSelectElement).value))">
            <option v-for="b in usableBots" :key="b.id" :value="b.id">{{ b.name }}</option>
          </select>
        </label>
        <label>
          Builder
          <select :value="botId(editing.config, 'builder')" @change="setBot(editing.config, 'builder', Number(($event.target as HTMLSelectElement).value))">
            <option v-for="b in usableBots" :key="b.id" :value="b.id">{{ b.name }}</option>
          </select>
        </label>
        <label>Fix attempts <input v-model.number="editing.config.limits.fixAttempts" type="number" min="1" max="10" /></label>
        <label>Phase time cap (minutes) <input v-model.number="editing.config.limits.phaseMinutes" type="number" min="5" max="2880" /></label>
        <label>Milestone every (phases) <input v-model.number="editing.config.milestoneEvery" type="number" min="1" max="10" /></label>
      </div>
      <p class="hint">
        Steps run Claude Code Bots for now, unattended: they may change files only inside the run's worktree and run only your checks, and anything
        else is refused rather than asked. Claude Code must already trust the repository.
      </p>

      <fieldset class="checks">
        <legend>Checks</legend>
        <p class="hint">Run in the worktree after every build, in this order. A task passes only when every check passes. Only you set them, here.</p>
        <div v-for="(c, i) in editing.config.checks" :key="i" class="check-row">
          <input v-model="c.name" placeholder="name" maxlength="40" />
          <input v-model="c.command" class="mono" placeholder="npm test" spellcheck="false" />
          <label class="secs"><input v-model.number="c.timeoutSecs" type="number" min="10" max="7200" /> s</label>
          <button type="button" class="quiet" :disabled="editing.config.checks.length <= 1" @click="removeCheck(i)">Remove</button>
        </div>
        <div class="check-actions">
          <button type="button" class="quiet" @click="addCheck">Add a check</button>
          <button type="button" class="quiet" :aria-expanded="pasting !== null" @click="pasting = pasting === null ? '' : null">Paste checks</button>
        </div>
        <div v-if="pasting !== null" class="paste">
          <label>
            Paste a table of checks, or one command per line
            <textarea v-model="pasting" class="mono" rows="7" spellcheck="false" placeholder="| deps | `npm install` | 900 s |" />
          </label>
          <p class="hint">
            A Markdown table works as it is, and so does one copied from a page or a terminal: the name, the command, then the time limit. A line with only a
            command is named after it, with 600 s.
          </p>
          <ul v-if="pasted.checks.length" class="paste-preview">
            <li v-for="(c, i) in pasted.checks" :key="i">
              <span>{{ c.name }}</span>
              <span class="mono">{{ c.command }}</span>
              <span class="muted">{{ c.timeoutSecs }} s</span>
            </li>
          </ul>
          <p v-for="(n, i) in pasted.notes" :key="`n${i}`" class="hint">{{ n }}</p>
          <p v-for="(p, i) in pasted.problems" :key="`p${i}`" class="hint warn">{{ p }}</p>
          <div class="actions">
            <span class="hint">
              {{
                pasted.checks.length
                  ? `They replace the ${plural(editing.config.checks.length, 'check')} above.`
                  : pasting.trim()
                    ? 'No checks found in that text.'
                    : 'Nothing pasted yet.'
              }}
            </span>
            <span class="grow" />
            <button type="button" class="quiet" @click="pasting = null">Cancel</button>
            <button type="button" class="primary" :disabled="!pasted.checks.length" @click="usePasted">
              Use {{ pasted.checks.length === 1 ? 'this check' : pasted.checks.length ? `these ${plural(pasted.checks.length, 'check')}` : 'these checks' }}
            </button>
          </div>
        </div>
      </fieldset>

      <label class="inline">
        <input v-model="editing.config.phoneStatusOnly" type="checkbox" />
        Phone: tell me a question is waiting, without its text (Settings → Phone)
      </label>

      <label v-if="flavor === 'dev'">
        Scripted steps (testing only)
        <input v-model="editing.config.script" class="mono" spellcheck="false" autocomplete="off" placeholder="a script file; empty runs real agents" />
      </label>

      <p v-if="error" class="hint warn">{{ error }}</p>
      <div class="actions">
        <span class="grow" />
        <button type="button" class="quiet" @click="editing = null">Cancel</button>
        <button type="submit" class="primary" :disabled="busy || !editing.name.trim() || !editing.folder.trim()">{{ editing.id === null ? 'Make run' : 'Save settings' }}</button>
      </div>
    </form>

    <div v-if="runs.length" class="table">
      <template v-for="r in runs" :key="r.id">
        <div class="row" :class="{ opened: open === r.id }">
          <button type="button" class="name" @click="open = open === r.id ? null : r.id">{{ r.name }}</button>
          <span class="what mono">{{ r.runBranch }} <span class="muted">from {{ r.baseBranch }}</span></span>
          <span v-if="r.state === 'running' && r.openQuestions" class="state waiting">Needs your answer</span>
          <span v-else class="state" :class="r.state">{{ STATE_TEXT[r.state] }}</span>
        </div>
        <div v-if="open === r.id" class="detail">
          <p v-if="r.note" class="note" :class="r.state">{{ r.note }}</p>
          <p class="hint mono">{{ r.worktreePath ?? r.repoRoot }}</p>
          <div class="actions">
            <button v-if="r.state === 'draft' || r.state === 'paused'" type="button" class="primary" :disabled="busy" @click="act(r, 'start')">
              {{ r.state === 'draft' ? 'Start' : 'Resume' }}
            </button>
            <button v-if="r.state === 'waiting'" type="button" class="primary" :disabled="busy" @click="act(r, 'approve')">Approve</button>
            <button v-if="r.state === 'running'" type="button" :disabled="busy" @click="act(r, 'pause')">Pause</button>
            <button v-if="['running', 'paused', 'waiting'].includes(r.state)" type="button" class="danger" :disabled="busy" @click="act(r, 'stop')">Stop</button>
            <button v-if="r.state !== 'running'" type="button" class="quiet" @click="editRun(r)">Settings</button>
            <span class="grow" />
            <template v-if="!['running', 'paused'].includes(r.state)">
              <template v-if="confirmingDelete === r.id">
                <span class="ask">Delete the run's record? Its branch, tags and worktree stay.</span>
                <button type="button" class="danger" :disabled="busy" @click="act(r, 'delete')">Delete</button>
                <button type="button" class="quiet" @click="confirmingDelete = null">Keep</button>
              </template>
              <button v-else type="button" class="quiet" @click="confirmingDelete = r.id">Delete…</button>
            </template>
          </div>

          <div v-if="r.questions.length" class="questions">
            <div v-for="q in r.questions.filter((x) => x.state === 'open')" :key="q.id" class="question">
              <div class="q-head">
                <span class="mono muted">{{ where(q) }}</span>
                <span class="badge" :class="q.class">{{ q.class === 'blocking' ? 'Blocking: the task waits for you' : `Timed: ${timeLeft(q)}` }}</span>
              </div>
              <p class="q-text">{{ q.question }}</p>
              <div v-if="q.options.length" class="q-options">
                <button
                  v-for="o in q.options"
                  :key="o"
                  type="button"
                  :class="{ chosen: draft(q).text === o }"
                  @click="draft(q).text = o"
                >
                  {{ o }}<span v-if="o === q.defaultAnswer" class="muted"> (default)</span>
                </button>
              </div>
              <div class="q-answer">
                <input v-model="draft(q).text" placeholder="Your answer" maxlength="2000" @keydown.enter="answer(q)" />
                <label class="check"><input v-model="draft(q).rule" type="checkbox" /> Make it a rule</label>
                <button type="button" class="primary" :disabled="busy || !draft(q).text.trim()" @click="answer(q)">Answer</button>
              </div>
              <p class="hint">A rule goes into the run's decisions.md, so no step asks this again.</p>
            </div>
            <details v-if="r.questions.some((x) => x.state !== 'open')" class="past">
              <summary>{{ r.questions.filter((x) => x.state !== 'open').length }} earlier question{{ r.questions.filter((x) => x.state !== 'open').length === 1 ? '' : 's' }}</summary>
              <ul>
                <li v-for="q in r.questions.filter((x) => x.state !== 'open')" :key="q.id">
                  <span class="mono muted">{{ where(q) }}</span> {{ q.question }}
                  <span class="muted"> · {{ outcome(q) }}</span>
                </li>
              </ul>
            </details>
          </div>

          <p v-if="!r.phases.length" class="hint">No phase has started yet.</p>
          <div v-for="p in r.phases" :key="p.number" class="phase">
            <div class="phase-head">
              <strong>P{{ String(p.number).padStart(2, '0') }} · {{ p.title }}</strong>
              <span class="state" :class="p.state">{{ p.state }}</span>
              <span v-if="p.tag" class="mono muted">{{ p.tag }}</span>
            </div>
            <ul v-if="p.tasks.length" class="tasks">
              <li v-for="t in p.tasks" :key="t.id">
                <span class="mono">{{ t.id }}</span> {{ t.title }}
                <span class="state" :class="t.status">{{ t.status }}</span>
                <span v-if="t.gates.length" class="muted"> · {{ t.gates.length }} check run{{ t.gates.length === 1 ? '' : 's' }}</span>
              </li>
            </ul>
            <ul class="steps">
              <li v-for="s in p.steps" :key="s.id" :class="s.state">
                <span class="when">{{ at(s.startedAtSecs) }}</span>
                <span class="label">{{ stepLabel(s) }}</span>
                <span class="who">{{ s.botName ?? '' }}<span v-if="model(s)" class="model mono muted">{{ model(s) }}</span></span>
                <span class="out">
                  {{ s.state === 'running' ? 'running…' : (s.note ?? s.state) }}
                  <span v-if="s.summary" class="summary">{{ s.summary }}</span>
                </span>
                <button v-if="s.sessionId !== null" type="button" class="quiet" @click="view.openSession(s.sessionId)">Open session</button>
                <ul v-if="s.checks.length" class="check-runs">
                  <li v-for="c in s.checks" :key="c.name" :class="{ failed: !c.passed }">
                    <span>{{ c.passed ? 'passed' : c.timedOut ? 'timed out' : 'failed' }}</span>
                    <span class="mono">{{ c.name }}</span>
                    <span class="muted">{{ seconds(c.durationMs) }}</span>
                    <span v-if="c.signature" class="sig">{{ c.signature.tests[0] ?? c.signature.firstError ?? '' }}</span>
                    <button type="button" class="quiet" @click="shownOutput = shownOutput === checkKey(s, c) ? null : checkKey(s, c)">
                      {{ shownOutput === checkKey(s, c) ? 'Hide output' : 'Output' }}
                    </button>
                    <pre v-if="shownOutput === checkKey(s, c)" class="output">{{ c.outputTail || '(no output)' }}</pre>
                  </li>
                </ul>
              </li>
            </ul>
          </div>
        </div>
      </template>
    </div>
    <p v-else class="empty">No runs yet. A run needs a repository with a roadmap, a planner and a builder Bot, and the checks a task must pass.</p>
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
h1 {
  margin: 0 0 6px;
  font-size: 28px;
}
.lead {
  margin: 0 0 22px;
  color: var(--text-2);
  max-width: 660px;
  line-height: 1.45;
}
.editor {
  display: grid;
  gap: 10px;
  padding: 16px;
  border: 1px solid var(--accent-line);
  border-radius: var(--radius-card);
  background: var(--surface-raised);
  margin-bottom: 22px;
}
.editor label {
  display: grid;
  gap: 4px;
  font-size: 12.5px;
  color: var(--text-2);
}
.editor label.inline {
  display: inline-flex;
  gap: 8px;
  align-items: center;
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
  gap: 10px;
}
.checks {
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  padding: 10px 12px;
  display: grid;
  gap: 8px;
}
.checks legend {
  font-size: 12.5px;
  color: var(--text-2);
  padding: 0 4px;
}
.check-row {
  display: grid;
  grid-template-columns: 140px 1fr 110px auto;
  gap: 8px;
  align-items: center;
}
.check-row .secs {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.check-row .secs input {
  width: 70px;
}
.check-actions {
  display: flex;
  gap: 8px;
}
.paste {
  display: grid;
  gap: 8px;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  background: var(--surface);
}
.paste textarea {
  resize: vertical;
  min-height: 120px;
  font-size: 12.5px;
}
.paste-preview {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  gap: 4px;
  font-size: 12.5px;
}
.paste-preview li {
  display: grid;
  grid-template-columns: 140px 1fr 70px;
  gap: 8px;
}
.paste-preview li > span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mono {
  font-family: var(--font-mono);
}
.hint {
  margin: 0;
  font-size: 12.5px;
  color: var(--text-muted);
  line-height: 1.45;
}
.hint.warn {
  color: var(--status-failed);
  white-space: pre-wrap;
}
.actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.ask {
  color: var(--text-2);
  font-size: 13px;
}
.grow {
  flex: 1;
}
.table {
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  overflow: hidden;
  background: var(--surface);
}
.row {
  display: grid;
  grid-template-columns: minmax(180px, 1fr) 2fr minmax(160px, auto);
  gap: 16px;
  align-items: center;
  padding: 14px 20px;
  border-bottom: 1px solid var(--border);
}
.row.opened {
  background: var(--surface-raised);
}
.name {
  justify-self: start;
  text-align: left;
  font-weight: 700;
  background: transparent;
  border: 0;
  padding: 0;
}
.what {
  color: var(--text-2);
  font-size: 13px;
}
.muted {
  color: var(--text-muted);
}
.state {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-muted);
  text-align: right;
}
.state.running,
.state.building,
.state.checking,
.state.planning {
  color: var(--accent);
}
.state.waiting,
.state.paused,
.state.blocked {
  color: var(--status-needs);
}
.state.stopped,
.state.failed {
  color: var(--status-failed);
}
.state.done,
.state.passed {
  color: var(--status-ready);
}
.detail {
  display: grid;
  gap: 12px;
  padding: 14px 20px 18px;
  border-bottom: 1px solid var(--border);
  background: var(--surface-raised);
}
.note {
  margin: 0;
  padding: 10px 12px;
  border-radius: var(--radius-card);
  border: 1px solid var(--border);
  font-size: 13px;
  line-height: 1.45;
  white-space: pre-wrap;
}
.note.stopped {
  border-color: var(--status-failed);
}
.note.waiting {
  border-color: var(--status-needs);
}
.phase {
  display: grid;
  gap: 6px;
  padding-top: 8px;
  border-top: 1px solid var(--border);
}
.phase-head {
  display: flex;
  gap: 10px;
  align-items: baseline;
}
.tasks,
.steps,
.check-runs {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  gap: 4px;
}
.tasks li {
  font-size: 13px;
  display: flex;
  gap: 8px;
  align-items: baseline;
}
.steps > li {
  display: grid;
  grid-template-columns: 70px 190px minmax(140px, 1fr) 2fr auto;
  gap: 10px;
  align-items: baseline;
  font-size: 13px;
}
.steps > li.failed .out {
  color: var(--status-failed);
}
.steps .model {
  display: block;
  font-size: 12px;
}
.steps .when {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-muted);
}
.check-runs {
  grid-column: 2 / -1;
}
.check-runs li {
  display: grid;
  grid-template-columns: 70px 120px 60px 1fr auto;
  gap: 10px;
  align-items: baseline;
  font-size: 12.5px;
}
.check-runs li.failed > span:first-child {
  color: var(--status-failed);
}
.sig {
  color: var(--text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.output {
  grid-column: 1 / -1;
  margin: 4px 0 6px;
  padding: 8px 10px;
  max-height: 320px;
  overflow: auto;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 6px;
  font-family: var(--font-mono);
  font-size: 12px;
  white-space: pre-wrap;
}
.empty {
  color: var(--text-muted);
}
.questions {
  display: grid;
  gap: 10px;
}
.question {
  display: grid;
  gap: 8px;
  padding: 12px 14px;
  border: 1px solid var(--status-needs);
  border-radius: var(--radius-card);
  background: var(--surface);
}
.q-head {
  display: flex;
  gap: 10px;
  align-items: baseline;
}
.badge {
  font-size: 12px;
  color: var(--status-needs);
}
.q-text {
  margin: 0;
  font-size: 14px;
  line-height: 1.45;
}
.q-options {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.q-options button.chosen {
  border-color: var(--accent);
  color: var(--accent);
}
.q-answer {
  display: grid;
  grid-template-columns: 1fr auto auto;
  gap: 10px;
  align-items: center;
}
.q-answer .check {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  font-size: 12.5px;
  color: var(--text-2);
}
.past summary {
  cursor: pointer;
  font-size: 12.5px;
  color: var(--text-2);
}
.past ul {
  list-style: none;
  margin: 6px 0 0;
  padding: 0;
  display: grid;
  gap: 4px;
  font-size: 12.5px;
}
.steps .summary {
  display: block;
  color: var(--text-2);
  font-size: 12.5px;
}
</style>
