<script setup lang="ts">
import type { Snapshot } from '../ipc'
defineProps<{ status: Snapshot }>()
</script>

<template>
  <div class="git-summary">
    <p><code>{{ status.changes.branch ?? 'Detached HEAD' }}</code> · {{ status.changes.files ? 'Uncommitted changes' : 'Clean' }}</p>
    <p v-if="status.changes.ahead !== null && status.changes.behind !== null" class="detail">
      {{ status.changes.ahead }} ahead · {{ status.changes.behind }} behind {{ status.changes.upstream }} (local refs)
    </p>
    <p v-else class="detail">{{ status.changes.upstream ? `Comparison unavailable for ${status.changes.upstream}` : 'No upstream comparison' }}</p>
    <p>
      {{ status.changes.files }} {{ status.changes.files === 1 ? 'file' : 'files' }}
      <template v-if="status.diff"> · +{{ status.diff.additions }} −{{ status.diff.deletions }}</template>
      <span v-else> · Line counts unavailable</span>
    </p>
    <p class="detail">
      {{ status.changes.staged }} staged · {{ status.changes.unstaged }} unstaged · {{ status.changes.untracked }} untracked
      <template v-if="status.diff?.binary"> · {{ status.diff.binary }} binary</template>
      <template v-if="status.changes.conflicts"> · {{ status.changes.conflicts }} conflicts</template>
    </p>
    <p class="detail">Lines: net tracked changes since HEAD; excludes untracked content, binary content and submodule contents.</p>
    <p v-if="status.diffNote" class="problem">{{ status.diffNote }}</p>
  </div>
</template>

<style scoped>
p { margin: 0 0 4px; overflow-wrap: anywhere; }
code { font-size: 12px; }
.detail { color: var(--text-muted); font-size: 11.5px; line-height: 1.5; }
.problem { color: var(--status-needs); }
</style>
