<script setup lang="ts">
/**
 * The Progress panel pinned at the top of Friendly (P16, D115, D120): the session's plan across its
 * turns, each step's status and rounded time, skill chips (estimates), "not timed" where Chorus never
 * saw a step in progress, the current step's tool calls, and where the newest run's time went (16-2).
 * Numbers come from the back end; this only words them and ticks the running step.
 */
import { computed, ref } from 'vue'
import type { SessionProgress, SessionSummary } from '../../ipc'
import { useNow } from '../../shell/now'
import { ATTENTION } from '../../shell/status'
import { timeLabel } from './conversation'
import { elapsedLabel, emptyLine, liveSecs, nowNext, skillChip, skillTitle, splitAria, splitLegend, splitNote, stepDuration, stepTime, stopped, toolGroupLabel, toolSummary, usualLabel, usualTitle } from './progress'

const props = defineProps<{ session: SessionSummary; progress: SessionProgress | null; error: string }>()
const now = useNow(1000)
const open = ref(true)
const toolsOpen = ref(false)

const p = computed(() => props.progress)
const rows = computed(() => p.value?.steps ?? [])
const hasRows = computed(() => rows.value.length > 0)
const line = computed(() => (p.value ? nowNext(p.value, now.value) : null))
const elapsed = computed(() => (props.session.live && props.session.runStartedAtSecs ? elapsedLabel(now.value - props.session.runStartedAtSecs) : ''))
const countLabel = computed(() => {
  const x = p.value
  if (!x || !x.total) return ''
  return `${x.done} of ${x.total} done`
})
const planTitle = computed(() => (p.value?.planAtSecs ? `Plan updated ${timeLabel(p.value.planAtSecs)}` : ''))
const title = computed(() => props.session.description || props.session.name)
const status = computed(() => ATTENTION[props.session.attention].label)
const working = computed(() => props.session.attention === 'working')
const thin = computed(() => rows.value.length > 30)
const tools = computed(() => p.value?.currentTools ?? [])
const toolsLabel = computed(() => toolSummary(tools.value))
const split = computed(() => p.value?.split ?? null)
const showSplit = computed(() => split.value !== null && open.value)
const canHide = computed(() => hasRows.value || split.value !== null)
const note = computed(() => (split.value ? splitNote(split.value) : null))
const splitParts = computed(() => (split.value ? splitLegend(split.value) : []))
const splitLabel = computed(() => (split.value ? splitAria(split.value) : ''))
const splitSince = computed(() => {
  const x = split.value
  if (!x) return ''
  const started = `Started ${timeLabel(x.startedAtSecs)}`
  return x.turns > 0 ? `${started} · turn ${x.turns}` : started
})
</script>

<template>
  <section class="panel" aria-label="Progress">
    <div class="head">
      <div class="titles">
        <span class="eyebrow">Progress</span>
        <h2>{{ title }}</h2>
      </div>
      <span class="pill" :class="`st-${session.attention}`"><span class="pill-dot" :class="{ pulse: working }" />{{ status }}</span>
      <div v-if="elapsed || countLabel" class="figures">
        <span v-if="elapsed" class="elapsed">{{ elapsed }}</span>
        <span v-if="countLabel" class="count" :title="planTitle">{{ countLabel }}</span>
      </div>
      <button v-if="canHide" class="toggle" :aria-expanded="open" @click="open = !open">{{ open ? 'Hide' : 'Show' }}</button>
    </div>

    <div v-if="hasRows" class="segments" :class="{ thin }" aria-hidden="true">
      <span v-for="(s, i) in rows" :key="i" class="seg" :class="[s.status, { pulse: s.status === 'in-progress' && s.runningSinceSecs !== null }]" />
    </div>

    <template v-if="hasRows && open">
      <ol class="steps scroll">
        <li v-for="(s, i) in rows" :key="i" class="row" :class="{ current: p?.current === i }">
          <div class="line">
            <span class="icon">
              <svg v-if="s.status === 'completed'" width="20" height="20" viewBox="0 0 20 20" role="img" aria-label="Done">
                <circle cx="10" cy="10" r="9" class="done-fill" stroke-width="1.5" />
                <path d="M6 10.4l2.6 2.6L14 7.6" fill="none" class="done-mark" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
              </svg>
              <svg v-else-if="s.status === 'in-progress'" width="20" height="20" viewBox="0 0 20 20" role="img" :aria-label="stopped(s) ? 'Stopped' : 'In progress'">
                <circle cx="10" cy="10" r="9" fill="none" class="cur-ring" stroke-width="1.5" />
                <circle cx="10" cy="10" r="4.5" class="cur-dot" :class="{ pulse: !stopped(s) }" />
              </svg>
              <svg v-else width="20" height="20" viewBox="0 0 20 20" role="img" aria-label="Up next">
                <circle cx="10" cy="10" r="8.5" fill="none" class="pend-ring" stroke-width="1.5" stroke-dasharray="3 3" />
              </svg>
            </span>
            <div class="name">
              <span class="text" :class="{ pending: s.status === 'pending' }">{{ s.text }}</span>
              <span v-if="s.kind === 'skill'" class="chip skill" title="A skill run outside the plan. Its time is an estimate.">skill</span>
              <template v-for="u in s.skills" :key="u.callId">
                <span v-if="s.kind === 'step'" class="chip skill" :title="skillTitle(u.name, liveSecs(u, now))">
                  <svg width="11" height="11" viewBox="0 0 12 12" aria-hidden="true"><path d="M6 1l1.4 3.1L10.7 4.5 8.2 6.7 8.9 10 6 8.3 3.1 10l.7-3.3L1.3 4.5l3.3-.4z" fill="currentColor" /></svg>{{ skillChip(u.name, liveSecs(u, now)) }}
                </span>
                <span v-if="u.usual" class="usual" role="note" :title="usualTitle(u.usual, u.name ?? 'skill')" :aria-label="usualTitle(u.usual, u.name ?? 'skill')">
                  <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M6 1.2L11 10.4H1z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" /><path d="M6 5v2.4M6 8.8v.1" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" /></svg>{{ usualLabel(u.usual, liveSecs(u, now)) }}
                </span>
              </template>
            </div>
            <span class="started">{{ s.startedAtSecs ? timeLabel(s.startedAtSecs) : '' }}</span>
            <span class="dur" :class="{ untimed: !s.timed, live: p?.current === i && !stopped(s) }">
              {{ stepTime(s, now) }}<span v-if="stopped(s)" class="stopped"> · stopped</span>
            </span>
          </div>
          <template v-if="p?.current === i && tools.length">
            <button class="tools-toggle quiet" :aria-expanded="toolsOpen" @click="toolsOpen = !toolsOpen">
              <span class="tools-sum">{{ toolsLabel }}</span>
              <svg class="chev" :class="{ up: toolsOpen }" width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M3 4.5L6 7.5 9 4.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
            </button>
            <ol v-if="toolsOpen" class="tools">
              <li v-for="(g, j) in tools" :key="j">
                <span class="tool-dot" :class="{ running: g.runningSinceSecs !== null, failed: g.failed > 0 }" />
                <span>{{ toolGroupLabel(g) }}<span v-if="g.failed" class="failed-note"> · {{ g.failed }} failed</span></span>
                <span class="tool-time">{{ stepDuration(liveSecs(g, now)) }}</span>
              </li>
            </ol>
          </template>
        </li>
      </ol>
      <p v-if="progress?.capped" class="note">Some plan updates weren't stored (500 a turn), so later times may be missing.</p>
    </template>

    <div v-else-if="hasRows && line" class="closed">
      <template v-if="line.now">
        <span class="now-dot pulse" />
        <span class="muted">Now:</span><strong>{{ line.now }}</strong><span class="mono">{{ line.nowSecs }}</span>
        <span v-if="line.next" class="muted">· Next: {{ line.next }}</span>
      </template>
      <span v-else-if="line.done" class="muted">All {{ progress?.total }} steps done</span>
      <span v-else-if="line.next" class="muted">Next: {{ line.next }}</span>
    </div>

    <div v-if="!hasRows && !error" class="closed empty">{{ emptyLine(session.agent) }}</div>

    <footer v-if="showSplit && split" class="split">
      <p v-if="note" class="split-none">{{ note }}</p>
      <template v-else>
        <div class="split-head">
          <h3 class="split-title">Where the time went</h3>
          <span class="split-since">{{ splitSince }}</span>
        </div>
        <div class="split-bar" role="img" :aria-label="splitLabel">
          <span v-for="part in splitParts" :key="part.key" class="part" :class="part.key" :style="{ flex: part.secs }" />
        </div>
        <ul class="legend">
          <li v-for="part in splitParts" :key="part.key" :class="part.key">
            <span class="swatch" :class="part.key" aria-hidden="true" />
            <span>{{ part.label }}</span>
            <span class="figure">{{ stepDuration(part.secs) }}</span>
          </li>
        </ul>
      </template>
    </footer>
    <p v-if="error" class="err" role="alert">Progress could not refresh: {{ error }}</p>
  </section>
</template>

<style scoped>
.panel { background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-card); overflow: hidden; box-shadow: 0 8px 28px rgb(0 0 0 / 0.28); }
.head { display: flex; flex-wrap: wrap; align-items: center; gap: 14px; padding: 16px 20px 12px; }
.titles { display: flex; flex-direction: column; gap: 3px; min-width: 0; flex: 1 1 200px; }
.eyebrow { font-size: 11px; font-weight: 600; letter-spacing: 0.08em; text-transform: uppercase; color: var(--accent); }
h2 { margin: 0; font-family: var(--font-heading); font-size: 19px; font-weight: 600; line-height: 1.25; overflow-wrap: anywhere; }
.pill { display: inline-flex; align-items: center; gap: 7px; font-size: 13px; font-weight: 600; color: var(--st); background: color-mix(in srgb, var(--st) 13%, transparent); border-radius: var(--radius-pill); padding: 5px 12px; }
.pill-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--st); }
.figures { display: flex; flex-direction: column; align-items: flex-end; gap: 1px; }
.elapsed { font-family: var(--font-mono); font-size: 17px; font-weight: 500; }
.count { font-size: 12px; color: var(--text-2); }
.toggle { border: 1px solid var(--border-strong); background: transparent; color: var(--text-2); border-radius: var(--radius-control); padding: 6px 12px; min-height: 34px; font-size: 13px; }
.toggle:hover { color: var(--text); }

.segments { display: flex; gap: 4px; padding: 0 20px 16px; }
.segments.thin { gap: 1px; }
.seg { flex: 1; height: 8px; border-radius: 4px; background: var(--border-strong); }
.seg.completed { background: var(--status-ready); }
.seg.in-progress { background: var(--accent); }

.steps { list-style: none; margin: 0; padding: 6px 12px 10px; border-top: 1px solid var(--border); max-height: min(28vh, 360px); }
.row { border-radius: 10px; margin: 2px 0; }
.row.current { background: color-mix(in srgb, var(--accent) 7%, transparent); }
.line { display: grid; grid-template-columns: 26px minmax(0, 1fr) 64px 112px; align-items: center; gap: 12px; padding: 9px 10px; }
.icon { display: inline-flex; justify-content: center; }
.done-fill { fill: color-mix(in srgb, var(--status-ready) 13%, transparent); stroke: var(--status-ready); }
.done-mark { stroke: var(--status-ready); }
.cur-ring { stroke: var(--accent-line); }
.cur-dot { fill: var(--accent); }
.pend-ring { stroke: var(--border-strong); }
.name { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-width: 0; }
.text { font-size: 14px; font-weight: 500; overflow-wrap: anywhere; }
.row.current .text { font-weight: 600; }
.text.pending { color: var(--text-2); }
.chip { display: inline-flex; align-items: center; gap: 5px; font-family: var(--font-mono); font-size: 11.5px; border-radius: 6px; padding: 2px 7px; }
.chip.skill { color: var(--tag-lavender); background: color-mix(in srgb, var(--tag-lavender) 14%, transparent); }
/* A usual-time marker (16-4, Main.dc.html's note chip): quiet, no motion, no alarm. */
.usual { display: inline-flex; align-items: center; gap: 5px; padding: 2px 9px; border-radius: var(--radius-pill); font-size: 12px; color: var(--status-needs); background: color-mix(in srgb, var(--status-needs) 13%, transparent); white-space: nowrap; }
.started { font-family: var(--font-mono); font-size: 12px; color: var(--text-2); text-align: right; }
.dur { font-family: var(--font-mono); font-size: 13px; text-align: right; white-space: nowrap; }
.dur.live { color: var(--accent); }
.dur.untimed { font-style: italic; color: var(--text-muted); }
.stopped { color: var(--text-muted); }

.tools-toggle { display: flex; align-items: center; gap: 6px; margin: -4px 10px 6px 48px; padding: 2px 4px; font-size: 12.5px; color: var(--text-2); max-width: calc(100% - 58px); }
.tools-sum { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.chev { flex: none; transition: transform 0.15s; }
.chev.up { transform: rotate(180deg); }
.tools { list-style: none; margin: 0 10px 8px 48px; padding: 4px 0 4px 14px; border-left: 2px solid var(--accent-line); }
.tools li { display: grid; grid-template-columns: 16px minmax(0, 1fr) 112px; align-items: center; gap: 10px; padding: 4px 0; font-size: 13px; }
.tool-dot { width: 8px; height: 8px; border-radius: 50%; margin-left: 4px; background: var(--status-ready); }
.tool-dot.running { background: var(--accent); }
.tool-dot.failed { background: var(--status-failed); }
.failed-note { color: var(--status-failed); }
.tool-time { font-family: var(--font-mono); font-size: 12px; color: var(--text-2); text-align: right; }

.note { margin: 0; padding: 0 20px 12px; font-size: 12px; color: var(--text-muted); }
.closed { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; padding: 10px 20px 14px; border-top: 1px solid var(--border); font-size: 13px; }
.closed.empty { color: var(--text-muted); }
.now-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--accent); }
.muted { color: var(--text-2); }
.mono { font-family: var(--font-mono); font-size: 12px; color: var(--text-2); }
.err { margin: 0; padding: 10px 20px 14px; font-size: 13px; color: var(--status-failed); }

.split { display: flex; flex-direction: column; gap: 10px; padding: 14px 20px 18px; border-top: 1px solid var(--border); }
.split-head { display: flex; flex-wrap: wrap; align-items: baseline; justify-content: space-between; gap: 4px 16px; }
.split-title { margin: 0; font-family: inherit; font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; color: var(--text-2); }
.split-since { font-size: 12px; color: var(--text-2); }
.split-bar { display: flex; gap: 2px; height: 12px; border-radius: 6px; overflow: hidden; }
.part { min-width: 3px; }
.part.working, .swatch.working { background: var(--status-working); }
.part.you, .swatch.you { background: var(--status-needs); }
.part.approval { background: repeating-linear-gradient(135deg, var(--status-approval) 0 4px, color-mix(in srgb, var(--status-approval) 40%, transparent) 4px 8px); }
.swatch.approval { background: repeating-linear-gradient(135deg, var(--status-approval) 0 3px, color-mix(in srgb, var(--status-approval) 40%, transparent) 3px 6px); }
.part.idle, .swatch.idle { background: var(--text-muted); }
.legend { display: flex; flex-wrap: wrap; gap: 8px 22px; margin: 0; padding: 0; list-style: none; font-size: 13px; }
.legend li { display: inline-flex; align-items: center; gap: 7px; }
.legend li.idle { color: var(--text-muted); }
.swatch { width: 10px; height: 10px; border-radius: 3px; flex: none; }
.figure { font-family: var(--font-mono); font-weight: 500; }
.split-none { margin: 0; font-size: 13px; color: var(--text-muted); }

.pulse { animation: progress-pulse 1.6s ease-in-out infinite; }
@keyframes progress-pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.45; } }

@media (max-width: 720px) {
  .line { grid-template-columns: 22px minmax(0, 1fr) 72px; }
  .started { display: none; }
  .tools-toggle, .tools { margin-left: 34px; }
}
</style>
