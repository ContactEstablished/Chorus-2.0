<script setup lang="ts">
// P18 (18-2): the project is with MausBot. Chorus watches the worktree's note for the baton to come
// back; Take back returns it now. When Chorus couldn't start MausBot, the banner says why and
// gives the folder and the message to paste.
import { computed } from 'vue'
import type { HandoffActive } from '../../ipc'
import { useHandoffStore } from '../../stores/handoff'
import { useNoticesStore } from '../../stores/notices'
import { useNow } from '../../shell/now'
import { activityLabel, clockLabel } from './handoff'

const props = defineProps<{ projectId: number; active: HandoffActive }>()
const store = useHandoffStore()
const notices = useNoticesStore()
const now = useNow(30_000)
const busy = computed(() => store.busy.has(props.projectId))
const activity = computed(() => activityLabel(props.active.activity, now.value))

async function copy(text: string, what: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text)
    notices.success(`${what} copied.`)
  } catch (e) {
    notices.error(`Chorus couldn’t copy the ${what.toLowerCase()}.`, e)
  }
}
</script>

<template>
  <section class="banner" aria-label="With MausBot">
    <div class="line">
      <span class="dot" aria-hidden="true" />
      <p class="what">
        With MausBot on <code>{{ active.branch }}</code> since {{ clockLabel(active.startedAtMs) }}<template v-if="active.botName"> · {{ active.botName }}</template>
        <span class="activity"> · {{ activity }}</span>
      </p>
      <button class="quiet" :disabled="busy" @click="store.takeBack(projectId)">{{ busy ? 'Taking back…' : 'Take back' }}</button>
    </div>
    <p v-if="active.task" class="muted">Task: {{ active.task }}</p>
    <template v-if="active.startProblem">
      <p class="warn">{{ active.startProblem }}</p>
      <p class="muted">In MausBot, start a new conversation with your bot in this folder, then paste the message.</p>
      <div class="copy">
        <button class="quiet" @click="copy(active.worktree, 'Folder')">Copy folder</button>
        <button v-if="active.message" class="quiet" @click="copy(active.message, 'Message')">Copy message</button>
        <code class="path" :title="active.worktree">{{ active.worktree }}</code>
      </div>
    </template>
    <p v-if="active.noteProblem" class="warn">{{ active.noteProblem }}</p>
  </section>
</template>

<style scoped>
.banner { background: color-mix(in srgb, var(--accent) 8%, var(--surface)); border: 1px solid var(--accent-line); border-radius: var(--radius-card); padding: 10px 16px; display: grid; gap: 6px; }
.line { display: flex; align-items: center; gap: 10px; }
.dot { width: 8px; height: 8px; border-radius: 50%; background: var(--accent); flex: none; }
.what { margin: 0; flex: 1; font-size: 13px; overflow-wrap: anywhere; }
.activity { color: var(--text-muted); }
code { font-family: var(--font-mono); font-size: 12px; }
.muted { margin: 0; font-size: 12.5px; color: var(--text-muted); }
.warn { margin: 0; font-size: 12.5px; color: var(--status-needs); overflow-wrap: anywhere; }
.copy { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
.path { color: var(--text-muted); overflow-wrap: anywhere; }
</style>
