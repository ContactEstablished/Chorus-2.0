<script setup lang="ts">
/**
 * P8's rows in session facts: the Bot the session started from, and what the current run's
 * instructions held (the Bot's, which notes, what was left out), or why none were given (D19).
 * Rendered inside SessionFacts' list.
 */
import { computed, ref, watch } from 'vue'
import { commands, unwrap, type SessionInstructions, type SessionSummary } from '../../ipc'

const props = defineProps<{ session: SessionSummary }>()
const facts = ref<SessionInstructions | null>(null)
const failed = ref<string | null>(null)

async function load(): Promise<void> {
  try {
    facts.value = await unwrap(commands.sessionInstructions(props.session.id))
    failed.value = null
  } catch (e) {
    failed.value = e instanceof Error ? e.message : String(e)
  }
}
// A new run reads notes afresh (decision 2), so its facts are read again.
watch(() => [props.session.id, props.session.runStartedAtSecs, props.session.live], load, { immediate: true })

const summary = computed(() => {
  const run = facts.value?.run
  if (!run) return facts.value?.unsupported ?? 'None given yet: the session hasn’t started a run since notes arrived.'
  if (!run.given) return run.reason ?? 'None: no Bot instructions, notes or skills apply.'
  const parts: string[] = []
  if (run.bot) parts.push('Bot')
  if (run.notes.length) parts.push(`${run.notes.length} note${run.notes.length === 1 ? '' : 's'}`)
  if (run.skills.length) parts.push(`${run.skills.length} skill${run.skills.length === 1 ? '' : 's'}`)
  return parts.join(' + ') || 'Given'
})
</script>

<template>
  <template v-if="facts?.bot">
    <dt>Bot</dt>
    <dd>{{ facts.bot.name }}{{ facts.bot.id === null ? ' (deleted; this session keeps its copy)' : '' }}</dd>
  </template>
  <template v-if="session.agent !== 'shell'">
    <dt>Instructions</dt>
    <dd :class="{ warn: facts?.run ? !facts.run.given && !!facts.run.reason : false }">
      <span>{{ failed ?? summary }}</span>
      <ul v-if="facts?.run?.notes.length" class="notes">
        <li v-for="n in facts.run.notes" :key="n.id">{{ n.title }}<span class="muted">{{ n.pinned ? ' · pinned' : '' }}{{ n.project ? '' : ' · all projects' }}</span></li>
      </ul>
      <span v-if="facts?.run?.leftOut" class="muted block">{{ facts.run.leftOut }} more left out: over the size limit.</span>
      <span v-if="facts?.run?.skills.length" class="muted block">Skills: {{ facts.run.skills.join(', ') }}</span>
      <span v-for="p in facts?.run?.skillProblems ?? []" :key="p" class="warn block">Skill not read: {{ p }}</span>
      <span v-if="facts?.run?.freshPrompt" class="muted block">Claude Code renders its system prompt afresh for this conversation, so edited notes apply.</span>
    </dd>
  </template>
</template>

<style scoped>
.notes {
  margin: 4px 0 0;
  padding-left: 16px;
}
.muted {
  color: var(--text-muted);
}
.block {
  display: block;
}
.warn {
  color: var(--status-failed);
}
</style>
