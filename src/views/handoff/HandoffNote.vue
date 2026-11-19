<script setup lang="ts">
// P18 (18-1): the project's hand-off note, `.chorus/handoff.md`. Chorus refreshes it after a
// session's turn ends; a note it can't read is shown with the reason and never overwritten.
import { computed, ref } from 'vue'
import type { HandoffView } from '../../ipc'
import { useHandoffStore } from '../../stores/handoff'
import { useNoticesStore } from '../../stores/notices'
import { useNow } from '../../shell/now'
import { authorLabel, earlierLabel, entryRows, ignoreLabel, updatedLabel } from './handoff'

const props = defineProps<{ view: HandoffView }>()
const store = useHandoffStore()
const notices = useNoticesStore()
const now = useNow(30_000)
const showAll = ref(false)

const newest = computed(() => props.view.entries[0] ?? null)
const older = computed(() => props.view.entries.slice(1))
const meta = computed(() => [props.view.updated ? updatedLabel(props.view.updated, now.value) : null, props.view.exists ? ignoreLabel(props.view.ignored) : null].filter(Boolean).join(' · '))
const busy = computed(() => store.busy.has(props.view.projectId))

async function open(): Promise<void> {
  try {
    await store.openNote(props.view.projectId)
  } catch (e) {
    notices.error('Chorus couldn’t open the note.', e)
  }
}
</script>

<template>
  <section class="card" aria-label="Hand-off note">
    <header class="head">
      <div class="titles">
        <h3>Hand-off note</h3>
        <p v-if="meta" class="meta" :title="view.notePath ?? undefined">{{ meta }}</p>
      </div>
      <slot name="actions" />
      <button class="quiet" :disabled="busy || !view.hasFolder" @click="store.refresh(view.projectId)">{{ busy ? 'Refreshing…' : 'Refresh' }}</button>
      <button class="quiet" :disabled="!view.exists" :title="view.exists ? 'Open .chorus/handoff.md' : 'There’s no note yet'" @click="open">Open file</button>
    </header>

    <p v-if="!view.hasFolder" class="muted">Set a folder for this project to keep a hand-off note.</p>
    <p v-else-if="view.problem" class="warn" role="alert" :title="view.notePath ?? undefined">{{ view.problem }}</p>
    <p v-if="view.hasFolder && !view.exists && !view.problem" class="muted">No note yet. Chorus writes one when a session in this project ends a turn.</p>

    <article v-if="newest" class="entry">
      <p class="heading">
        <span class="mono">{{ newest.heading }}</span>
        <span v-if="authorLabel(newest)" class="badge">{{ authorLabel(newest) }}</span>
      </p>
      <dl v-if="entryRows(newest).length" class="rows">
        <template v-for="r in entryRows(newest)" :key="r.label">
          <dt>{{ r.label }}</dt>
          <dd>{{ r.value }}</dd>
        </template>
      </dl>
      <p v-for="(o, i) in newest.other" :key="i" class="other">{{ o }}</p>
    </article>

    <template v-if="older.length">
      <button class="quiet more" :aria-expanded="showAll" @click="showAll = !showAll">{{ earlierLabel(older.length, showAll) }}</button>
      <template v-if="showAll">
        <article v-for="(e, i) in older" :key="i" class="entry older">
          <p class="heading">
            <span class="mono">{{ e.heading }}</span>
            <span v-if="authorLabel(e)" class="badge">{{ authorLabel(e) }}</span>
          </p>
          <dl v-if="entryRows(e).length" class="rows">
            <template v-for="r in entryRows(e)" :key="r.label">
              <dt>{{ r.label }}</dt>
              <dd>{{ r.value }}</dd>
            </template>
          </dl>
          <p v-for="(o, j) in e.other" :key="j" class="other">{{ o }}</p>
        </article>
      </template>
    </template>
  </section>
</template>

<style scoped>
.card { background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-card); padding: 10px 16px 12px; }
.head { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
.titles { flex: 1 1 200px; min-width: 0; }
h3 { margin: 0; font-size: 13px; font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; color: var(--text-2); }
.meta { margin: 2px 0 0; font-size: 12px; color: var(--text-muted); }
.muted { margin: 8px 0 0; font-size: 12.5px; color: var(--text-muted); }
.warn { margin: 8px 0 0; font-size: 12.5px; color: var(--status-needs); overflow-wrap: anywhere; }
.entry { margin-top: 10px; padding-top: 8px; border-top: 1px solid var(--border); }
.entry.older { opacity: 0.85; }
.heading { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin: 0 0 6px; font-size: 12px; color: var(--text-2); }
.mono { font-family: var(--font-mono); }
.badge { font-size: 11px; padding: 1px 7px; border-radius: var(--radius-pill); color: var(--accent); background: color-mix(in srgb, var(--accent) 12%, transparent); }
.rows { display: grid; grid-template-columns: 56px minmax(0, 1fr); gap: 3px 10px; margin: 0; font-size: 13px; }
dt { color: var(--text-muted); font-size: 12px; padding-top: 1px; }
dd { margin: 0; overflow-wrap: anywhere; }
.other { margin: 4px 0 0; font-size: 12px; color: var(--text-muted); overflow-wrap: anywhere; }
.more { margin-top: 8px; font-size: 12px; }
</style>
