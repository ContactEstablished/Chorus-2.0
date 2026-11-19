<script setup lang="ts">
// P18 (18-2): the baton came back. What MausBot wrote (or Chorus's entry from git), its commits and
// files, and whether the branch is merged. Continue from here puts a summary in a session's
// composer; nothing is sent.
import { computed, ref } from 'vue'
import type { HandoffReturn, Project } from '../../ipc'
import { useComposerStore } from '../../stores/composer'
import { useHandoffStore } from '../../stores/handoff'
import { useNoticesStore } from '../../stores/notices'
import { useSessionsStore } from '../../stores/sessions'
import { useViewStore } from '../../stores/view'
import { branchState, clockLabel, entryRows, fileStat, moreLabel, returnTitle } from './handoff'

const props = defineProps<{ project: Project; returned: HandoffReturn }>()
const store = useHandoffStore()
const sessions = useSessionsStore()
const composer = useComposerStore()
const notices = useNoticesStore()
const view = useViewStore()
const busy = computed(() => store.busy.has(props.project.id))

/** The project's open sessions, newest first, for Continue from here. */
const targets = computed(() => sessions.list.filter((s) => s.projectId === props.project.id && s.status !== 'closed').slice().sort((a, b) => b.latestAtSecs - a.latestAtSecs))
const choosing = ref(false)
const target = ref<number | null>(null)

function choose(): void {
  target.value = targets.value[0]?.id ?? null
  choosing.value = true
}

async function continueIn(): Promise<void> {
  const id = target.value
  if (id === null) return
  try {
    const text = await store.continueText(props.project.id)
    await composer.addSuggestion(id, text)
    choosing.value = false
    view.openSession(id)
  } catch (e) {
    notices.error('Chorus couldn’t add the summary to that session.', e)
  }
}
</script>

<template>
  <section class="card" aria-label="Back from MausBot">
    <header class="head">
      <div class="titles">
        <h3>{{ returnTitle(project.name, returned.returnedBy) }}</h3>
        <p class="meta">{{ clockLabel(returned.returnedAtMs) }} · <code>{{ returned.branch }}</code> · {{ branchState(returned) }}</p>
      </div>
      <button class="quiet" :disabled="busy" @click="store.dismiss(project.id)">Dismiss</button>
    </header>

    <article v-for="(e, i) in returned.entries" :key="i" class="entry">
      <p class="heading">
        <span class="mono">{{ e.heading }}</span>
        <span v-if="returned.fromGit && i === 0" class="badge">from git</span>
      </p>
      <dl v-if="entryRows(e).length" class="rows">
        <template v-for="r in entryRows(e)" :key="r.label">
          <dt>{{ r.label }}</dt>
          <dd>{{ r.value }}</dd>
        </template>
      </dl>
    </article>

    <div v-if="returned.commits.length" class="list">
      <p class="label">Commits</p>
      <p v-for="c in returned.commits" :key="c.sha" class="row"><code>{{ c.sha }}</code> {{ c.subject }}</p>
      <p v-if="moreLabel(returned.moreCommits, 'commit', 'commits')" class="muted">{{ moreLabel(returned.moreCommits, 'commit', 'commits') }}</p>
    </div>
    <div v-if="returned.files.length" class="list">
      <p class="label">Files</p>
      <p v-for="f in returned.files" :key="f.path" class="row"><code class="path">{{ f.path }}</code> <span class="stat">{{ fileStat(f) }}</span></p>
      <p v-if="moreLabel(returned.moreFiles, 'file', 'files')" class="muted">{{ moreLabel(returned.moreFiles, 'file', 'files') }}</p>
    </div>
    <p v-if="returned.problem" class="warn">{{ returned.problem }}</p>
    <p v-if="!returned.worktreeExists" class="muted">The worktree folder is gone; the branch is still there.</p>

    <div class="actions">
      <template v-if="choosing">
        <select v-if="targets.length" v-model="target" aria-label="Session to continue in">
          <option v-for="s in targets" :key="s.id" :value="s.id">{{ s.name }}</option>
        </select>
        <p v-else class="muted">Start a session in this project first.</p>
        <button v-if="targets.length" class="primary" :disabled="target === null" @click="continueIn">Add to its message</button>
        <button class="quiet" @click="choosing = false">Cancel</button>
      </template>
      <button v-else class="primary" @click="choose">Continue from here</button>
      <slot name="actions" />
    </div>
  </section>
</template>

<style scoped>
.card { background: var(--surface); border: 1px solid var(--accent-line); border-radius: var(--radius-card); padding: 10px 16px 12px; display: grid; gap: 8px; }
.head { display: flex; align-items: center; gap: 8px; }
.titles { flex: 1; min-width: 0; }
h3 { margin: 0; font-size: 14px; font-weight: 600; }
.meta { margin: 2px 0 0; font-size: 12px; color: var(--text-muted); overflow-wrap: anywhere; }
code, .mono { font-family: var(--font-mono); font-size: 12px; }
.entry { padding-top: 6px; border-top: 1px solid var(--border); }
.heading { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin: 0 0 6px; font-size: 12px; color: var(--text-2); }
.badge { font-size: 11px; padding: 1px 7px; border-radius: var(--radius-pill); color: var(--text-muted); background: color-mix(in srgb, var(--text-muted) 14%, transparent); }
.rows { display: grid; grid-template-columns: 56px minmax(0, 1fr); gap: 3px 10px; margin: 0; font-size: 13px; }
dt { color: var(--text-muted); font-size: 12px; padding-top: 1px; }
dd { margin: 0; overflow-wrap: anywhere; }
.list { display: grid; gap: 2px; }
.label { margin: 0; font-size: 12px; color: var(--text-muted); }
.row { margin: 0; font-size: 12.5px; overflow-wrap: anywhere; }
.path { color: var(--text); }
.stat { font-family: var(--font-mono); font-size: 12px; color: var(--text-muted); }
.muted { margin: 0; font-size: 12.5px; color: var(--text-muted); }
.warn { margin: 0; font-size: 12.5px; color: var(--status-needs); overflow-wrap: anywhere; }
.actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
</style>
