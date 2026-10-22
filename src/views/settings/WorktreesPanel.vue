<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { commands, unwrap, type WorktreeEntry } from '../../ipc'
import GitSummary from '../../components/GitSummary.vue'

const trees = ref<WorktreeEntry[]>([])
const error = ref<string | null>(null)
const busy = ref(false)
const showRemoved = ref(false)
const shown = computed(() => trees.value.filter((w) => showRemoved.value || !w.removed))
async function refresh(): Promise<void> {
  if (busy.value) return
  busy.value = true
  error.value = null
  try { trees.value = await unwrap(commands.worktreesList(null)) }
  catch (e) { trees.value = []; error.value = String(e) }
  finally { busy.value = false }
}
onMounted(refresh)
</script>

<template>
  <section class="worktrees" aria-label="Worktrees">
    <div class="head">
      <h2>Worktrees</h2>
      <button :disabled="busy" @click="refresh">{{ busy ? 'Checking Git…' : 'Refresh' }}</button>
    </div>
    <p class="hint">Worktrees created by Chorus. To continue one, choose its repository in New session, then Existing worktree.</p>
    <label class="hint"><input v-model="showRemoved" type="checkbox" /> Show removed worktrees</label>
    <p v-if="error" class="problem">{{ error }}</p>
    <p v-else-if="!shown.length && !busy" class="hint">No {{ showRemoved ? '' : 'retained ' }}worktrees.</p>
    <ul :aria-busy="busy">
      <li v-for="w in shown" :key="w.id">
        <div class="head"><strong>{{ w.branch }}</strong><span class="hint">{{ w.removed ? 'Removed / missing' : w.openSessions ? `${w.openSessions} open session(s)` : 'Retained' }}</span></div>
        <p class="path">{{ w.path }}</p>
        <p class="path hint">Repository: {{ w.repoRoot }}</p>
        <p v-if="w.problem" class="problem">{{ w.problem }}</p>
        <GitSummary v-if="w.status" :status="w.status" />
      </li>
    </ul>
  </section>
</template>

<style scoped>
.worktrees { margin-top: 30px; padding-top: 24px; border-top: 1px solid var(--border); font-size: 13px; }
.head { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
h2 { margin: 0; font-size: 17px; }
.hint { color: var(--text-muted); line-height: 1.5; }
ul { list-style: none; padding: 0; display: grid; gap: 10px; }
li { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; padding: 14px; min-width: 0; }
strong, .path { overflow-wrap: anywhere; }
.path { font-family: var(--font-mono); font-size: 11.5px; margin: 6px 0; }
.problem { color: var(--status-needs); overflow-wrap: anywhere; }
</style>
