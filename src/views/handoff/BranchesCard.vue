<script setup lang="ts">
// P18 (18-3, D131): branches that have sat unused for 30 days, and MausBot's branches. Keep hides
// one for 30 days; Delete removes it (and its worktree, when nothing there would be lost), asking
// again before unmerged commits go. Chorus never deletes a branch on its own, and only deletes the
// commit the card showed: if the branch moved since, the back end refuses and the card reloads.
import { computed, onMounted, ref, watch } from 'vue'
import type { BranchView } from '../../ipc'
import { useHandoffStore } from '../../stores/handoff'
import { branchLine, canDelete, deleteQuestion } from './handoff'

const props = defineProps<{ projectId: number }>()
const store = useHandoffStore()
const report = computed(() => store.branchReports.get(props.projectId) ?? null)
const busy = ref<string | null>(null)
/** The unmerged branch whose second confirmation is showing. */
const confirming = ref<string | null>(null)
const checking = ref(false)
/** Why the card's last action failed, beside the branches it's about. */
const error = ref('')

async function load(id: number): Promise<void> {
  error.value = (await store.loadBranches(id)) ?? ''
}

onMounted(() => void load(props.projectId))
watch(
  () => props.projectId,
  (id) => {
    confirming.value = null
    void load(id)
  },
)

async function check(): Promise<void> {
  checking.value = true
  error.value = (await store.checkBranches(props.projectId)) ?? ''
  checking.value = false
}

async function keep(b: BranchView): Promise<void> {
  busy.value = b.name
  error.value = (await store.keepBranch(props.projectId, b.name)) ?? ''
  busy.value = null
}

async function remove(b: BranchView, confirmed: boolean): Promise<void> {
  if (!b.merged && !confirmed) {
    confirming.value = b.name
    return
  }
  busy.value = b.name
  error.value = (await store.deleteBranch(props.projectId, b.name, b.tip, confirmed)) ?? ''
  busy.value = null
  confirming.value = null
}
</script>

<template>
  <section class="card" aria-label="Branches">
    <header class="head">
      <div class="titles">
        <h3>Branches</h3>
        <p class="meta">Unused for 30 days, and MausBot’s. Nothing is deleted unless you say so.</p>
      </div>
      <button class="quiet" :disabled="checking" @click="check">{{ checking ? 'Checking…' : 'Check now' }}</button>
    </header>
    <p v-if="error" class="warn" role="alert">{{ error }}</p>
    <p v-if="report?.problem" class="warn">{{ report.problem }}</p>
    <p v-else-if="report && !report.branches.length" class="muted">No branches unused for 30 days.</p>
    <ul v-if="report?.branches.length" class="list">
      <li v-for="b in report.branches" :key="b.name" class="row">
        <div class="what">
          <code>{{ b.name }}</code>
          <span class="line">{{ branchLine(b, report.base) }}</span>
          <span v-if="b.worktree" class="path" :title="b.worktree">{{ b.worktree }}</span>
          <span v-if="b.redacted" class="line muted">Its name looks like a secret, so Chorus hides it. Delete or keep it with git.</span>
        </div>
        <template v-if="confirming === b.name && b.ahead !== null">
          <p class="ask">{{ deleteQuestion({ name: b.name, ahead: b.ahead }, report.base) }}</p>
          <button class="danger" :disabled="busy === b.name" @click="remove(b, true)">Delete for good</button>
          <button class="quiet" @click="confirming = null">Cancel</button>
        </template>
        <template v-else-if="!b.redacted">
          <button v-if="b.stale" class="quiet" :disabled="busy === b.name" @click="keep(b)">Keep 30 days</button>
          <button v-if="canDelete(b)" class="quiet" :disabled="busy === b.name" @click="remove(b, false)">Delete</button>
        </template>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.card { background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-card); padding: 10px 16px 12px; display: grid; gap: 8px; }
.head { display: flex; align-items: center; gap: 8px; }
.titles { flex: 1; min-width: 0; }
h3 { margin: 0; font-size: 13px; font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; color: var(--text-2); }
.meta { margin: 2px 0 0; font-size: 12px; color: var(--text-muted); }
.muted { margin: 0; font-size: 12.5px; color: var(--text-muted); }
.warn { margin: 0; font-size: 12.5px; color: var(--status-needs); overflow-wrap: anywhere; }
.list { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
.row { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding-top: 6px; border-top: 1px solid var(--border); }
.what { flex: 1 1 260px; min-width: 0; display: grid; gap: 1px; }
code { font-family: var(--font-mono); font-size: 12.5px; overflow-wrap: anywhere; }
.line { font-size: 12px; color: var(--text-2); }
.line.muted { color: var(--text-muted); }
.path { font-family: var(--font-mono); font-size: 11.5px; color: var(--text-muted); overflow-wrap: anywhere; }
.ask { margin: 0; flex-basis: 100%; font-size: 12.5px; color: var(--status-needs); }
</style>
