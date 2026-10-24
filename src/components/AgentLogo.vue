<script setup lang="ts">
import type { AgentKind } from '../ipc'
import { AGENT_LABELS } from '../raw/labels'
import claude from '../assets/agents/claude.png'
import codex from '../assets/agents/codex.png'
import opencode from '../assets/agents/opencode.png'
import grok from '../assets/agents/grok.png'
import gemini from '../assets/agents/gemini.png'
import aider from '../assets/agents/aider.png'
import shell from '../assets/agents/shell.png'

defineProps<{ agent: AgentKind }>()
const logos: Record<AgentKind, string> = { claude, codex, opencode, grok, gemini, aider, shell }
</script>

<template>
  <span class="agent-chip agent-logo" :class="`agent-${agent}`" :title="AGENT_LABELS[agent]">
    <img :src="logos[agent]" :alt="AGENT_LABELS[agent]" />
  </span>
</template>

<style scoped>
.agent-logo { width: 28px; height: 28px; padding: 0; flex: none; border-radius: 7px; }
.agent-logo img { width: 22px; height: 22px; object-fit: contain; }
</style>
