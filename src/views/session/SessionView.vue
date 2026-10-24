<script setup lang="ts">
/**
 * One session (glacier-session-raw.png): the strip, a header with its status and controls, the
 * live Raw terminal, and a panel with what it's waiting for, its prompts and its facts. Friendly
 * mode is P6; Raw is the agent's own terminal.
 */
import { computed, ref, watch } from 'vue'
import Icon from '../../components/Icon.vue'
import codexLogo from '../../assets/agents/codex.png'
import { compactPath } from '../../shell/compactPath'
import SessionTerminal from '../../raw/SessionTerminal.vue'
import RawSearch from '../../raw/RawSearch.vue'
import ExportTranscript from './ExportTranscript.vue'
import AddressChip from './AddressChip.vue'
import FleetRoster from './FleetRoster.vue'
import SessionStrip from './SessionStrip.vue'
import SessionFacts from './SessionFacts.vue'
import PromptHistory from './PromptHistory.vue'
import ActionChannels from './ActionChannels.vue'
import FriendlyTimeline from './FriendlyTimeline.vue'
import { commands, type Attention, type SessionSummary, type WorktreeClose } from '../../ipc'
import { terminalFor } from '../../components/terminals'
import { useSessionsStore } from '../../stores/sessions'
import { useProjectsStore } from '../../stores/projects'
import { useKeysStore } from '../../stores/keys'
import { useViewStore } from '../../stores/view'
import { useLayoutStore } from '../../stores/layout'
import { useAppStore } from '../../stores/app'
import { useNoticesStore } from '../../stores/notices'
import { AGENT_LABELS } from '../../raw/labels'
import { AGENT_CHIPS, ATTENTION, shortPath } from '../../shell/status'

const props = defineProps<{ sessionId: number }>()
const mode = ref<'friendly' | 'raw'>('raw')

const sessions = useSessionsStore()
const projects = useProjectsStore()
const keys = useKeysStore()
const view = useViewStore()
const layout = useLayoutStore()
const app = useAppStore()
const notices = useNoticesStore()

const s = computed<SessionSummary | null>(() => sessions.byId[props.sessionId] ?? null)
const project = computed(() => (s.value?.projectId == null ? null : (projects.byId.get(s.value.projectId) ?? null)))
const openingFolder = ref(false)
async function openFolder(): Promise<void> {
  if (!s.value || openingFolder.value) return
  openingFolder.value = true
  try {
    const result = await commands.sessionOpenFolder(s.value.id)
    if (result.status === 'error') notices.error('Could not open the session folder.', result.error)
  } catch (e) {
    notices.error('Could not open the session folder. Restart Chorus if its backend has not been updated yet.', e)
  } finally { openingFolder.value = false }
}

// A session that closes (here or elsewhere) leaves its view.
watch(s, (now) => {
  if (!now) view.forgetSession(props.sessionId)
})

/** DESIGN.md's readiness panel, worded for P4: answers go through the terminal until P6. */
const READINESS: Record<Attention, { title: string; text: string }> = {
  approval: { title: 'Waiting on you.', text: 'Review the request in Friendly. If its reply channel is unavailable, approve or deny in Raw.' },
  input: { title: 'Waiting on you.', text: 'Answer the active question card in Friendly, or use Raw when no reply channel is available.' },
  failed: { title: 'Agent stopped.', text: 'History is intact. Restart runs it again, resuming its conversation where the agent allows it.' },
  working: { title: 'Agent is busy.', text: 'It is working on a turn. What you type goes straight to its terminal.' },
  ready: { title: 'Ready for the next step.', text: 'The last turn is finished. Type your next instruction in the terminal.' },
  running: { title: 'Running.', text: "Chorus has no event from this agent, so it can't tell whether it's busy. Watch the terminal." },
  done: { title: 'Session finished.', text: 'It exited cleanly. Restart starts it again, in the same conversation where the agent allows it.' },
}
const readiness = computed(() => {
  const x = s.value
  if (!x) return null
  const r = READINESS[x.attention]
  if (x.attention === 'failed' && x.live) return { title: 'Its last turn failed.', text: 'The agent is still running. Tell it how to recover in the terminal.' }
  if (x.attention === 'running' && !x.hasEventChannel) {
    return { title: 'Running.', text: `${AGENT_LABELS[x.agent]} has no event channel, so Chorus can't tell whether it's busy. Watch the terminal.` }
  }
  if (x.attention === 'working' && x.noSignal) return { title: 'No signal.', text: 'Working, but neither an event nor any output for a while. It may be stuck, or thinking.' }
  // Claude Code and Codex take instructions from Friendly's composer (D78); the rest only in Raw.
  if (x.agent === 'claude' || x.agent === 'codex') {
    if (x.attention === 'ready') return { title: r.title, text: 'The last turn is finished. Send your next instruction from Friendly, or type it in Raw.' }
    if (x.attention === 'working') return { title: r.title, text: 'It is working on a turn. Instructions you queue in Friendly are sent when it pauses.' }
  }
  return r
})

// Name and note, edited in place.
const nameDraft = ref<string | null>(null)
const noteDraft = ref<string | null>(null)
async function saveName(): Promise<void> {
  if (s.value && nameDraft.value !== null && (await sessions.rename(s.value.id, nameDraft.value))) nameDraft.value = null
}
async function saveNote(): Promise<void> {
  if (s.value && noteDraft.value !== null && (await sessions.setDescription(s.value.id, noteDraft.value.trim() || null))) noteDraft.value = null
}

// Controls.
const busy = ref(false)
const confirmingClose = ref(false)
// What Close would do to the session's worktree, fetched when Close is asked for (D53).
const closeWorktree = ref<WorktreeClose | null>(null)
const askingPin = ref(false)
const pin = ref('')
async function restart(): Promise<void> {
  if (!s.value) return
  busy.value = true
  await sessions.restart(s.value.id)
  busy.value = false
}
async function askClose(): Promise<void> {
  const x = s.value
  if (!x) return
  closeWorktree.value = null
  if (x.worktree && !x.worktree.removed) {
    busy.value = true
    const r = await commands.sessionCloseCheck(x.id).catch(() => null)
    busy.value = false
    closeWorktree.value = r?.status === 'ok' ? r.data.worktree : null
  }
  confirmingClose.value = true
}
// Removal is offered only when nothing would be lost and no other session works there. A folder
// deleted outside Chorus has nothing left to lose: removing it clears git's record of it.
const canRemoveWorktree = computed(() => {
  const w = closeWorktree.value
  return !!w && (w.clean === true || w.gone) && w.others === 0
})
const closeQuestion = computed(() => {
  const w = closeWorktree.value
  const base = `Close ${s.value?.name}? Its process ends.`
  if (!w) return base
  if (w.others) return `${base} Its worktree stays: ${w.others === 1 ? 'another session works' : `${w.others} other sessions work`} in it.`
  if (w.gone) return `${base} Its worktree's folder is already gone; removing it clears git's record of it.`
  if (w.clean === false) return `${base} Its worktree has uncommitted changes, so it stays.`
  if (w.clean === null) return `${base} Its worktree stays (${w.note ?? 'git couldn’t check it'}).`
  return `${base} Its worktree has nothing uncommitted.`
})
async function close(removeWorktree = false): Promise<void> {
  if (!s.value) return
  const name = s.value.name
  busy.value = true
  confirmingClose.value = false
  const report = await sessions.close(s.value.id, removeWorktree)
  busy.value = false
  if (report) {
    const spared = report.spared ? ` ${report.spared} windowed process${report.spared === 1 ? ' was' : 'es were'} left open (for example a browser it opened).` : ''
    const worktree = report.worktreeNote ? ` ${report.worktreeNote}` : ''
    notices.info(`Closed ${name}.${spared}${worktree}`)
  }
}
async function toggleLock(): Promise<void> {
  const x = s.value
  if (!x) return
  if (x.locked && keys.pinSet && !askingPin.value) {
    askingPin.value = true
    return
  }
  const ok = await sessions.setLocked(x.id, !x.locked, x.locked ? pin.value || null : null)
  pin.value = ''
  if (ok) askingPin.value = false
}

// The terminal.
const attached = ref(false)
const size = ref<{ cols: number; rows: number } | null>(null)
// Raw search (P7).
const rawTerm = ref<InstanceType<typeof SessionTerminal> | null>(null)
const rawSearch = ref<InstanceType<typeof RawSearch> | null>(null)
const searchOpen = ref(false)
/** Opens the search bar, or puts the focus back in it when it is already open. */
function openSearch(): void {
  if (searchOpen.value) rawSearch.value?.focus()
  searchOpen.value = true
}
watch(
  () => props.sessionId,
  () => {
    attached.value = false
    size.value = null
    nameDraft.value = null
    noteDraft.value = null
    confirmingClose.value = false
    askingPin.value = false
  },
)
async function send(bytes: string): Promise<void> {
  const res = await commands.sessionWrite(props.sessionId, bytes)
  if (res.status === 'error') notices.error(res.error)
  terminalFor(props.sessionId)?.focus()
}
</script>

<template>
  <section v-if="s" class="session">
    <SessionStrip :current="s.id" />

    <header class="head">
      <button class="back" title="Back" aria-label="Back" @click="view.goBack()"><Icon name="back" :size="16" /></button>
      <span class="agent-chip big" :class="`agent-${s.agent}`" :title="AGENT_LABELS[s.agent]">
        <img v-if="s.agent === 'codex'" :src="codexLogo" alt="Codex" />
        <template v-else>{{ AGENT_CHIPS[s.agent] }}</template>
      </span>
      <div class="titles">
        <form v-if="nameDraft !== null" class="rename" @submit.prevent="saveName">
          <input v-model="nameDraft" maxlength="80" aria-label="Session name" autocomplete="off" @keyup.esc="nameDraft = null" />
          <button class="primary" type="submit">Save</button>
          <button type="button" class="quiet" @click="nameDraft = null">Cancel</button>
        </form>
        <h1 v-else class="name" title="Click to rename" @click="nameDraft = s.name">{{ s.name }}</h1>
        <p class="where">
          <button class="folder-link quiet" :disabled="openingFolder" :title="`Open folder in File Explorer: ${s.cwd}`" aria-label="Open session folder in File Explorer" @click="openFolder"><Icon name="folder" :size="15" /></button>
          <span v-if="project" class="project-tag" :style="{ '--tag': project.colour }">{{ project.name }}</span>
          <code :title="s.cwd">{{ compactPath(shortPath(s.cwd, app.info?.homeDir ?? null)) }}</code>
          <span v-if="s.providerName || s.options.model" class="mono">{{ s.providerName }}<template v-if="s.providerName && s.options.model"> / </template>{{ s.options.model }}</span>
          <AddressChip v-if="s.agent === 'claude'" :session-id="s.id" />
        </p>
      </div>
      <span class="pill" :class="`st-${s.attention}`">
        <span class="dot" :class="{ pulse: s.attention === 'working' && !s.noSignal }" />{{ ATTENTION[s.attention].label }}
      </span>
      <div class="modes" role="radiogroup" aria-label="View">
        <button role="radio" :aria-checked="mode === 'friendly'" :class="{ on: mode === 'friendly' }" @click="mode = 'friendly'">Friendly</button>
        <button role="radio" :aria-checked="mode === 'raw'" :class="{ on: mode === 'raw' }" @click="mode = 'raw'">Raw</button>
      </div>
      <button
        class="details-toggle quiet"
        :class="{ on: layout.detailsOpen }"
        :title="layout.detailsOpen ? 'Hide session details' : 'Show session details'"
        :aria-label="layout.detailsOpen ? 'Hide session details' : 'Show session details'"
        :aria-expanded="layout.detailsOpen"
        aria-controls="session-details"
        @click="layout.detailsOpen = !layout.detailsOpen"
      ><Icon name="panelRight" :size="18" /></button>
    </header>

    <div class="controls">
      <p class="note" :class="{ empty: !s.description }">
        <template v-if="noteDraft !== null">
          <input v-model="noteDraft" maxlength="500" placeholder="What this session is for" aria-label="Note" @keyup.enter="saveNote" @keyup.esc="noteDraft = null" />
          <button class="primary" @click="saveNote">Save</button>
          <button class="quiet" @click="noteDraft = null">Cancel</button>
        </template>
        <button v-else class="quiet notebtn" @click="noteDraft = s.description ?? ''">{{ s.description ?? 'Add a note on what this session is for…' }}</button>
      </p>
      <div class="actions">
        <ExportTranscript :session-id="s.id" />
        <template v-if="askingPin">
          <input v-model="pin" type="password" placeholder="PIN" autocomplete="off" aria-label="PIN" @keyup.enter="toggleLock" />
          <button @click="toggleLock">Unlock</button>
          <button class="quiet" @click="askingPin = false">Cancel</button>
        </template>
        <button v-else class="quiet" :title="s.locked ? 'Unlock: allow Close and Restart' : 'Lock: block Close and Restart'" @click="toggleLock">
          {{ s.locked ? 'Unlock' : 'Lock' }}
        </button>
        <button class="quiet" :disabled="busy || s.locked" :title="s.locked ? 'Unlock it first' : 'End this run and start it again'" @click="restart">Restart</button>
        <button class="quiet danger" :disabled="busy || s.locked || confirmingClose" :title="s.locked ? 'Unlock it first' : 'End the session'" @click="askClose">Close</button>
      </div>
    </div>
    <div v-if="confirmingClose" class="confirm" role="alertdialog" aria-label="Close the session">
      <span class="ask">{{ closeQuestion }}</span>
      <template v-if="canRemoveWorktree">
        <button class="danger" :disabled="busy" :title="`Removes ${closeWorktree?.path}; the branch ${closeWorktree?.branch} is kept`" @click="close(true)">Close and remove worktree</button>
        <button :disabled="busy" @click="close(false)">Close, keep worktree</button>
      </template>
      <button v-else class="danger" :disabled="busy" @click="close(false)">Close</button>
      <button class="quiet" @click="confirmingClose = false">Keep</button>
    </div>
    <p v-if="s.note" class="warn run-note">{{ s.note }}</p>

    <div class="body" :class="{ 'details-hidden': !layout.detailsOpen }">
      <FriendlyTimeline v-if="mode === 'friendly'" :session="s" @raw="mode = 'raw'" />
      <div v-else class="raw">
        <div class="rawbar">
          <span class="att" :class="{ on: attached && s.live }">
            <span class="led" />{{ !s.live ? 'not running' : attached ? 'attached' : 'attaching…' }}
          </span>
          <span v-if="size" class="mono muted">{{ size.cols }}×{{ size.rows }}</span>
          <span class="mono muted">scrollback 10k</span>
          <span class="spacer" />
          <button class="key" title="Find in the terminal (Ctrl+Shift+F)" aria-label="Find in the terminal" @click="openSearch"><Icon name="search" :size="13" /></button>
          <button class="key" :disabled="!s.live" title="Send Ctrl+C" @click="send('\x03')">^C</button>
          <button class="key" :disabled="!s.live" title="Send Escape (interrupts most agents)" @click="send('\x1b')">Esc</button>
        </div>
        <RawSearch v-if="searchOpen" ref="rawSearch" :key="`search-${s.id}`" :term="rawTerm" @close="searchOpen = false" />
        <p v-if="s.scrollbackError" class="warn">{{ s.scrollbackError }}</p>
        <div class="term">
          <SessionTerminal
            ref="rawTerm"
            :key="s.id"
            :session-id="s.id"
            :live="s.live"
            searchable
            @search="openSearch"
            @attached="(a) => (attached = a)"
            @size="(cols, rows) => (size = { cols, rows })"
            @error="(m) => notices.error(m)"
          />
        </div>
        <p class="hint">
          Keystrokes go straight to the agent's terminal. Pasted or dictated text is never sent on its own: press Enter when
          it's right.
        </p>
      </div>

      <aside v-show="layout.detailsOpen" id="session-details" class="panel scroll" aria-label="Session details">
        <ActionChannels :session-id="s.id" :live="s.live" />
        <div v-if="readiness" class="ready" :class="`st-${s.attention}`">
          <p class="rt"><span class="dot" />{{ readiness.title }}</p>
          <p class="rx">{{ readiness.text }}</p>
        </div>
        <PromptHistory :session="s" />
        <SessionFacts :session="s" />
        <FleetRoster />
      </aside>
    </div>
  </section>
  <section v-else class="gone">
    <p>That session is closed.</p>
    <button @click="view.go('overview')">Back to the Overview</button>
  </section>
</template>

<style scoped>
.session {
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.head {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 14px 20px 8px;
  min-width: 0;
}
.back {
  width: 32px;
  height: 32px;
  padding: 0;
  display: grid;
  place-items: center;
  background: transparent;
  flex: none;
}
.agent-chip.big {
  height: 32px;
  width: 32px;
  min-width: 32px;
  border-radius: 8px;
  font-size: 12px;
}
.agent-chip.big img { width: 24px; height: 24px; object-fit: contain; }
.folder-link { display: grid; place-items: center; flex: none; width: 20px; height: 20px; padding: 0; color: var(--text-muted); }
.folder-link:hover { color: var(--accent); }
.titles {
  flex: 1;
  min-width: 0;
}
.name {
  margin: 0;
  font-size: 20px;
  cursor: text;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.rename {
  display: flex;
  gap: 8px;
}
.rename input {
  flex: 1;
  min-width: 0;
  font-size: 15px;
}
.where {
  margin: 4px 0 0;
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  color: var(--text-muted);
  min-width: 0;
  white-space: nowrap;
}
.where code,
.mono {
  font-family: var(--font-mono);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  min-width: 0;
}
.pill {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  border-radius: var(--radius-pill);
  background: color-mix(in srgb, var(--st) 14%, transparent);
  color: var(--st);
  font-weight: 600;
  font-size: 13px;
  white-space: nowrap;
}
.pill.st-done,
.pill.st-running {
  color: var(--text-2);
  background: var(--surface-chip);
}
.modes {
  display: inline-flex;
  padding: 3px;
  gap: 2px;
  border-radius: 10px;
  background: var(--surface);
  border: 1px solid var(--border);
  flex: none;
}
.modes button {
  border: 0;
  background: transparent;
  padding: 5px 14px;
  color: var(--text-2);
  font-weight: 600;
  font-size: 13px;
}
.modes button.on {
  background: var(--accent-soft);
  color: var(--accent);
  box-shadow: inset 0 0 0 1px var(--accent-line);
}
.controls {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 20px 10px 66px;
  border-bottom: 1px solid var(--border);
  min-width: 0;
}
.note {
  flex: 1;
  margin: 0;
  display: flex;
  gap: 8px;
  min-width: 0;
}
.note input {
  flex: 1;
  min-width: 0;
}
.notebtn {
  padding: 3px 6px;
  margin-left: -6px;
  font-size: 13px;
  color: var(--text-2);
  text-align: left;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  max-width: 100%;
}
.note.empty .notebtn {
  color: var(--text-muted);
}
.actions {
  display: flex;
  align-items: center;
  gap: 4px;
  flex: none;
}
.actions button {
  padding: 4px 10px;
  font-size: 13px;
}
.actions input {
  width: 100px;
  padding: 4px 8px;
}
.confirm {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 6px;
  margin: 0 20px 8px;
  padding: 8px 10px;
  border-radius: var(--radius-control);
  border: 1px solid var(--border-strong);
  background: var(--surface-raised);
}
.confirm button {
  padding: 4px 10px;
  font-size: 13px;
}
.ask {
  margin-right: auto;
  font-size: 12.5px;
  color: var(--text-2);
}
.body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(0, 1fr) 320px;
}
.body.details-hidden { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr); }
.details-toggle { display: grid; place-items: center; flex: none; width: 32px; height: 32px; padding: 0; }
.details-toggle.on { color: var(--accent); background: var(--accent-soft); }
.raw {
  display: flex;
  flex-direction: column;
  min-height: 0;
  padding: 10px 16px 8px 20px;
  gap: 8px;
}
.rawbar {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 12px;
}
.att {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-family: var(--font-mono);
  color: var(--text-muted);
}
.att.on {
  color: var(--status-ready);
}
.led {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
}
.muted {
  color: var(--text-muted);
}
.spacer {
  flex: 1;
}
.key {
  font-family: var(--font-mono);
  font-size: 12px;
  padding: 3px 9px;
  border-radius: 7px;
  background: transparent;
}
.warn {
  margin: 0;
  padding: 6px 10px;
  border-radius: 8px;
  background: color-mix(in srgb, var(--status-needs) 10%, transparent);
  color: var(--status-needs);
  font-size: 12.5px;
}
.run-note {
  margin: 0 20px 8px;
}
.term {
  flex: 1;
  min-height: 0;
  border: 1px solid var(--border);
  border-radius: 12px;
  overflow: hidden;
}
.hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-muted);
}
.panel {
  border-left: 1px solid var(--border);
  padding: 16px 18px;
  display: flex;
  flex-direction: column;
  gap: 22px;
  min-height: 0;
}
.ready {
  padding: 12px 14px;
  border-radius: 12px;
  border: 1px solid color-mix(in srgb, var(--st) 55%, transparent);
  background: color-mix(in srgb, var(--st) 8%, transparent);
}
.ready.st-done,
.ready.st-running {
  border-color: var(--border-strong);
  background: var(--surface);
}
.rt {
  margin: 0 0 4px;
  display: flex;
  align-items: center;
  gap: 8px;
  font-weight: 600;
  color: var(--st);
}
.ready.st-done .rt,
.ready.st-running .rt {
  color: var(--text);
}
.rx {
  margin: 0;
  font-size: 13px;
  color: var(--text-2);
  line-height: 1.5;
}
.gone {
  padding: 40px;
  color: var(--text-2);
}
@media (max-width: 980px) {
  .body {
    grid-template-columns: 1fr;
    grid-template-rows: minmax(0, 1fr) auto;
  }
  .panel {
    border-left: 0;
    border-top: 1px solid var(--border);
    max-height: 220px;
  }
  .where .mono,
  .where .sep {
    display: none;
  }
  .controls {
    padding-left: 20px;
  }
}
</style>
