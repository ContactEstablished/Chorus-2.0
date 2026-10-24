<script setup lang="ts">
/**
 * Automations (P8): a Bot run on a schedule while Chorus runs. Each run creates an ordinary
 * session. Rows show the Bot, schedule and last run; a row opens its history, Run now, Pause and
 * Edit. Times missed while Chorus was closed, and runs skipped because the previous one was still
 * busy, are listed with their reason (decision 3).
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { commands, unwrap, type Automation, type AutomationRun, type AutomationSave, type Schedule } from '../ipc'
import { useBotsStore } from '../stores/bots'
import { useProjectsStore } from '../stores/projects'
import { useViewStore } from '../stores/view'
import { useNoticesStore } from '../stores/notices'
import { automationStatus, runOutcome } from './botText'

const bots = useBotsStore()
const projects = useProjectsStore()
const view = useViewStore()
const notices = useNoticesStore()

const list = ref<Automation[]>([])
const open = ref<number | null>(null)
const runs = ref<AutomationRun[]>([])
const editing = ref<AutomationSave | null>(null)
const kind = ref<Schedule['kind']>('daily')
const minutes = ref(60)
const time = ref('09:00')
const days = ref<number[]>([0, 1, 2, 3, 4])
const error = ref<string | null>(null)
const busy = ref(false)
// Deleting an automation (and its run history) can't be undone, so it asks once, in place.
const confirmingDelete = ref(false)
let timer: ReturnType<typeof setInterval> | null = null

const DAY_NAMES = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']

// The list is polled; a lasting failure is said once, not every 10 s.
let failing = false
async function load(): Promise<void> {
  try {
    list.value = await unwrap(commands.automationsList())
    if (open.value !== null) runs.value = await unwrap(commands.automationRuns(open.value))
    failing = false
  } catch (e) {
    if (!failing) notices.error("Couldn't load automations.", e)
    failing = true
  }
}
onMounted(() => {
  void load()
  void bots.load()
  if (!projects.list.length) void projects.load()
  timer = setInterval(() => void load(), 10_000)
})
onUnmounted(() => {
  if (timer) clearInterval(timer)
})

const usableBots = computed(() => bots.list.filter((b) => !b.archived))

function schedule(): Schedule {
  const [h, m] = time.value.split(':').map(Number)
  const hour = h !== undefined && Number.isFinite(h) ? h : 0
  const minute = m !== undefined && Number.isFinite(m) ? m : 0
  if (kind.value === 'every') return { kind: 'every', minutes: Number(minutes.value) }
  if (kind.value === 'daily') return { kind: 'daily', hour, minute }
  return { kind: 'weekdays', days: [...days.value].sort(), hour, minute }
}

function startNew(): void {
  error.value = null
  confirmingDelete.value = false
  kind.value = 'daily'
  minutes.value = 60
  time.value = '09:00'
  days.value = [0, 1, 2, 3, 4]
  editing.value = { id: null, name: '', botId: usableBots.value[0]?.id ?? 0, prompt: '', schedule: schedule(), cwd: '', projectId: null, enabled: true }
}

function edit(a: Automation): void {
  error.value = null
  confirmingDelete.value = false
  const s = a.schedule
  kind.value = s.kind
  if (s.kind === 'every') minutes.value = s.minutes
  else time.value = `${String(s.hour).padStart(2, '0')}:${String(s.minute).padStart(2, '0')}`
  if (s.kind === 'weekdays') days.value = [...s.days]
  editing.value = { id: a.id, name: a.name, botId: a.botId ?? usableBots.value[0]?.id ?? 0, prompt: a.prompt, schedule: s, cwd: a.cwd, projectId: a.projectId, enabled: a.enabled }
}

async function save(): Promise<void> {
  if (!editing.value) return
  busy.value = true
  error.value = null
  try {
    if (kind.value !== 'every' && !/^\d{1,2}:\d{2}$/.test(time.value)) throw new Error('Choose a time of day.')
    await unwrap(commands.automationSave({ ...editing.value, schedule: schedule() }))
    editing.value = null
    await load()
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = false
  }
}

async function remove(): Promise<void> {
  const id = editing.value?.id
  if (id === null || id === undefined) return
  try {
    await unwrap(commands.automationDelete(id))
  } catch (e) {
    // Keep the editor open so the reason shows.
    error.value = e instanceof Error ? e.message : String(e)
    return
  }
  confirmingDelete.value = false
  editing.value = null
  if (open.value === id) open.value = null
  await load()
}

async function toggle(a: Automation): Promise<void> {
  try {
    await unwrap(commands.automationSetEnabled(a.id, !a.enabled))
    await load()
  } catch (e) {
    notices.error(`Couldn't ${a.enabled ? 'pause' : 'resume'} ${a.name}.`, e)
  }
}

async function runNow(a: Automation): Promise<void> {
  busy.value = true
  try {
    const r = await unwrap(commands.automationRunNow(a.id))
    if (r.outcome === 'started') notices.success(`${a.name} started a session.`)
    else notices.error(`${a.name} didn't start.`, r.note)
    open.value = a.id
    await load()
  } catch (e) {
    notices.error(`Couldn't run ${a.name}.`, e)
  } finally {
    busy.value = false
  }
}

async function toggleOpen(a: Automation): Promise<void> {
  open.value = open.value === a.id ? null : a.id
  runs.value = []
  if (open.value === null) return
  try {
    runs.value = await unwrap(commands.automationRuns(a.id))
  } catch (e) {
    notices.error(`Couldn't load ${a.name}'s run history.`, e)
  }
}

function at(secs: number | null): string {
  if (!secs) return ''
  const t = new Date(secs * 1000)
  const sameDay = t.toDateString() === new Date().toDateString()
  return sameDay ? t.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : t.toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
}

function lastText(a: Automation): { text: string; cls: string } {
  return automationStatus(a, at)
}

function outcomeText(r: AutomationRun): string {
  return runOutcome(r)
}
</script>

<template>
  <section class="page">
    <header class="head">
      <div>
        <h1>Automations</h1>
        <p class="lead">
          Recurring agent work. Each run creates a normal session you can open from the Overview. Schedules run only while Chorus is open; a time
          missed while it was closed is recorded, not caught up.
        </p>
      </div>
      <button type="button" class="primary" :disabled="!usableBots.length" :title="usableBots.length ? '' : 'Make a Bot first'" @click="startNew">
        New automation
      </button>
    </header>

    <form v-if="editing" class="editor" @submit.prevent="save">
      <div class="grid">
        <label>Name <input v-model="editing.name" maxlength="80" autocomplete="off" /></label>
        <label>
          Bot
          <select v-model="editing.botId">
            <option v-for="b in usableBots" :key="b.id" :value="b.id">{{ b.name }}</option>
          </select>
        </label>
        <label>
          Schedule
          <select v-model="kind">
            <option value="every">Every N minutes</option>
            <option value="daily">Daily</option>
            <option value="weekdays">On chosen days</option>
          </select>
        </label>
        <label v-if="kind === 'every'">Minutes (15 or more) <input v-model.number="minutes" type="number" min="15" step="5" /></label>
        <label v-else>Time <input v-model="time" type="time" /></label>
      </div>
      <div v-if="kind === 'weekdays'" class="days" role="group" aria-label="Days">
        <label v-for="(d, i) in DAY_NAMES" :key="d"><input v-model="days" type="checkbox" :value="i" /> {{ d }}</label>
      </div>
      <label>Folder <input v-model="editing.cwd" placeholder="C:\Projects\app" spellcheck="false" autocomplete="off" class="mono" /></label>
      <label>
        Project
        <select v-model="editing.projectId">
          <option :value="null">No project</option>
          <option v-for="p in projects.usable" :key="p.id" :value="p.id">{{ p.name }}</option>
        </select>
      </label>
      <label>First prompt <textarea v-model="editing.prompt" rows="4" placeholder="What the session should do when it starts" /></label>
      <p class="hint">
        The prompt is recorded in the session's history as an automation prompt and passed to Claude Code when it starts. Automations start Claude
        Code Bots for now. Make sure Claude Code already trusts the folder, or the run waits at its trust question.
      </p>
      <label class="check"><input v-model="editing.enabled" type="checkbox" /> Enabled</label>
      <p v-if="error" class="hint warn">{{ error }}</p>
      <div class="actions">
        <template v-if="editing.id !== null">
          <template v-if="confirmingDelete">
            <span class="ask">Delete it and its run history?</span>
            <button type="button" class="danger" :disabled="busy" @click="remove">Delete</button>
            <button type="button" class="quiet" @click="confirmingDelete = false">Keep</button>
          </template>
          <button v-else type="button" class="danger" @click="confirmingDelete = true">Delete…</button>
        </template>
        <span class="grow" />
        <button type="button" class="quiet" @click="editing = null">Cancel</button>
        <button type="submit" class="primary" :disabled="busy || !editing.name.trim()">Save automation</button>
      </div>
    </form>

    <div v-if="list.length" class="table">
      <template v-for="a in list" :key="a.id">
        <div class="row" :class="{ opened: open === a.id }">
          <button type="button" class="name" @click="toggleOpen(a)">{{ a.name }}</button>
          <span class="what">{{ a.botName ?? 'No Bot' }} · {{ a.scheduleText }}<span v-if="a.runsThisWeek" class="muted"> · {{ a.runsThisWeek }} run{{ a.runsThisWeek === 1 ? '' : 's' }} this week</span></span>
          <span class="last" :class="lastText(a).cls">{{ lastText(a).text }}</span>
        </div>
        <div v-if="open === a.id" class="detail">
          <p v-if="a.problem" class="hint warn">{{ a.problem }}</p>
          <p class="hint">{{ a.prompt }}</p>
          <p class="hint mono">{{ a.cwd }}<template v-if="a.enabled && a.nextRunAtSecs"> · next run {{ at(a.nextRunAtSecs) }}</template></p>
          <div class="actions">
            <button type="button" class="primary" :disabled="busy" @click="runNow(a)">Run now</button>
            <button type="button" @click="toggle(a)">{{ a.enabled ? 'Pause' : 'Resume' }}</button>
            <button type="button" class="quiet" @click="edit(a)">Edit</button>
          </div>
          <ul v-if="runs.length" class="runs">
            <li v-for="r in runs" :key="r.id" :class="r.outcome">
              <span class="when">{{ at(r.dueAtSecs) }}</span>
              <span class="trig">{{ r.trigger === 'manual' ? 'Run now' : 'Scheduled' }}</span>
              <span class="out">{{ outcomeText(r) }}</span>
              <button v-if="r.sessionId !== null" type="button" class="quiet" @click="view.openSession(r.sessionId)">Open session</button>
            </li>
          </ul>
          <p v-else class="hint">No runs yet.</p>
        </div>
      </template>
    </div>
    <p v-else class="empty">No automations yet. An automation runs a Bot with a first prompt on a schedule, and each run is an ordinary session.</p>
  </section>
</template>

<style scoped>
.page {
  padding: 28px 40px;
  max-width: 1040px;
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
  max-width: 640px;
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
.editor label.check,
.days label {
  display: inline-flex;
  gap: 6px;
  align-items: center;
}
.days {
  display: flex;
  gap: 12px;
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
  gap: 10px;
}
.mono {
  font-family: var(--font-mono);
}
textarea {
  resize: vertical;
  font-family: inherit;
}
.hint {
  margin: 0;
  font-size: 12.5px;
  color: var(--text-muted);
  line-height: 1.45;
}
.hint.warn {
  color: var(--status-failed);
}
.actions {
  display: flex;
  align-items: center;
  gap: 8px;
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
  grid-template-columns: minmax(200px, 1.2fr) 2fr minmax(140px, auto);
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
  font-size: 13.5px;
}
.muted {
  color: var(--text-muted);
}
.last {
  text-align: right;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-muted);
}
.last.ok {
  color: var(--status-ready);
}
.last.paused {
  color: var(--status-needs);
}
.last.bad {
  color: var(--status-failed);
}
.detail {
  display: grid;
  gap: 10px;
  padding: 14px 20px 18px;
  border-bottom: 1px solid var(--border);
  background: var(--surface-raised);
}
.runs {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  gap: 4px;
}
.runs li {
  display: grid;
  grid-template-columns: 120px 90px 1fr auto;
  gap: 12px;
  align-items: center;
  font-size: 13px;
}
.runs .when,
.runs .trig {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-muted);
}
.runs li.failed .out {
  color: var(--status-failed);
}
.runs li.missed .out,
.runs li.skipped .out {
  color: var(--status-needs);
}
.empty {
  color: var(--text-muted);
}
</style>
