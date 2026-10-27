import type { AgentKind } from '../ipc'
import claude from '../assets/agents/claude.png'
import codex from '../assets/agents/codex.png'
import opencode from '../assets/agents/opencode.png'
import grok from '../assets/agents/grok.png'
import gemini from '../assets/agents/gemini.png'
import aider from '../assets/agents/aider.png'
import shell from '../assets/agents/shell.png'

/** One logo per agent kind, for anywhere a chip should show the model rather than its initials. */
export const AGENT_LOGOS: Record<AgentKind, string> = { claude, codex, opencode, grok, gemini, aider, shell }
