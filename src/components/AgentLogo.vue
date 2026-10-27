<script setup lang="ts">
import type { AgentKind } from '../ipc'
import { AGENT_LABELS } from '../raw/labels'
import { AGENT_LOGOS } from './agentLogos'

defineProps<{ agent: AgentKind }>()
</script>

<template>
  <span class="agent-chip agent-logo" :class="`agent-${agent}`" :title="AGENT_LABELS[agent]">
    <img :src="AGENT_LOGOS[agent]" :alt="AGENT_LABELS[agent]" />
  </span>
</template>

<style scoped>
.agent-logo { width: 28px; height: 28px; padding: 0; flex: none; border-radius: 7px; }
.agent-logo img { width: 22px; height: 22px; object-fit: contain; }
</style>
