<script setup lang="ts">
import type { PlanSnapshot } from '../../ipc'
defineProps<{ plan: PlanSnapshot | null; currentRunId: number | null }>()
const labels = { pending: 'Pending', 'in-progress': 'In progress', completed: 'Completed' }
</script>

<template>
  <section class="plan" aria-label="Recorded plan">
    <h3>Plan</h3>
    <template v-if="plan">
      <p v-if="plan.incomplete" class="note" role="alert">Some task updates could not be captured. This checklist may be incomplete; check Raw.</p>
      <p v-if="currentRunId === null" class="note">Last recorded plan. This session has no live run.</p>
      <p v-else-if="plan.runId !== currentRunId" class="note">Last recorded plan from an earlier run. It may no longer describe the agent's current work.</p>
      <ol v-if="plan.steps.length">
        <li v-for="step in plan.steps" :key="step.id" :class="step.status">
          <span class="state">{{ labels[step.status] }}</span><span>{{ step.title }}</span>
        </li>
      </ol>
      <p v-else class="note">The agent cleared its checklist.</p>
    </template>
    <p v-else class="note">No structured plan recorded yet. A plan written as ordinary text stays in the conversation.</p>
  </section>
</template>

<style scoped>
.plan { padding: 16px 18px; margin: 16px 0; background: var(--surface); border: 1px solid var(--border); border-radius: 14px; }
h3 { font-size: 14px; margin: 0 0 10px; }
ol { list-style: none; padding: 0; margin: 0; display: grid; gap: 10px; }
li { display: flex; gap: 12px; align-items: baseline; font-size: 13px; overflow-wrap: anywhere; }
.state { flex: 0 0 78px; font-size: 11px; color: var(--text-muted); }
.in-progress .state { color: var(--status-working); }
.completed .state { color: var(--status-ready); }
.note { color: var(--text-muted); line-height: 1.5; font-size: 12px; margin: 6px 0; }
</style>
