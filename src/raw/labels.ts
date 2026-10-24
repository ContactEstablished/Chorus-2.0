import type { Activity, AgentInfo, AgentKind, Effort, Permission, ProviderKind, SessionSummary, VersionFit } from '../ipc'

export const AGENT_LABELS: Record<AgentKind, string> = {
  shell: 'Shell',
  claude: 'Claude Code',
  codex: 'Codex',
  opencode: 'opencode',
  grok: 'Grok',
  gemini: 'Gemini CLI',
  aider: 'Aider',
}

export const KIND_LABELS: Record<ProviderKind, string> = {
  openrouter: 'OpenRouter',
  gateway: 'OpenAI-compatible gateway',
  jev: 'Jev (TypeSafe)',
  ollama: 'Ollama (local)',
}

/** Dollars: $1.00, or more places for small amounts ($0.0042). */
export function money(usd: number | null | undefined): string {
  if (usd === null || usd === undefined) return '?'
  return `$${usd >= 0.1 || usd === 0 ? usd.toFixed(2) : usd.toPrecision(2)}`
}

export function when(secs: number | null): string {
  return secs ? new Date(secs * 1000).toLocaleString() : 'never'
}

/** What a running session's agent is doing, from its events (ARCHITECTURE §6). */
export const ACTIVITY_LABELS: Record<Activity, string> = {
  unknown: 'running',
  working: 'working',
  'awaiting-approval': 'waiting for your approval',
  'needs-answer': 'needs your answer',
  ready: 'ready for the next step',
  failed: 'its last turn failed',
}

export const EFFORT_LABELS: Record<Effort, string> = {
  fast: 'Fast',
  balanced: 'Balanced',
  deep: 'Deep',
  max: 'Max',
}

export const PERMISSION_LABELS: Record<Permission, string> = {
  manual: 'Manual',
  auto: 'Auto',
  'accept-edits': 'Accept edits',
  plan: 'Plan',
  'full-access': 'Full access',
}

/** "Claude Code 2.1.300, checked" or why it can't be launched, for the launch form. */
export function detectionText(a: AgentInfo): string {
  const d = a.detection
  if (!d) return 'Looking for it…'
  if (!d.found) return d.problem ?? 'Not found'
  if (a.kind === 'shell') return 'Windows command prompt'
  return versionText(d.version, d.fit, a.verifiedVersion) + (d.problem ? ` (${d.problem})` : '')
}

export function versionText(version: string | null, fit: VersionFit | null, verified: string | null): string {
  if (!version) return 'version unknown, so its options are untested'
  switch (fit) {
    case 'verified':
      return `${version}, the version Chorus checked`
    case 'newer':
      return `${version}, newer than ${verified ?? '?'}: options may not apply`
    case 'older':
      return `${version}, older than ${verified ?? '?'}: options may not apply`
    default:
      return version
  }
}

/** The session header's summary of what it asked its agent for. */
export function optionsText(s: Pick<SessionSummary, 'options'>): string {
  const parts: string[] = []
  if (s.options.effort) parts.push(`${EFFORT_LABELS[s.options.effort]} effort`)
  if (s.options.permission) parts.push(PERMISSION_LABELS[s.options.permission])
  if (s.options.model) parts.push(s.options.model)
  return parts.join(' · ')
}

/**
 * What the newest turn really ran with, read back from the agent: "glm-5.3-flash-free · high · plan agent"
 * (opencode), "claude-opus-5-5 · xhigh effort" (Claude Code, which may not record an effort).
 */
export function observedText(s: SessionSummary): string | null {
  const o = s.observed
  if (!o) return null
  const effort = s.agent === 'claude' ? (o.variant ? `${o.variant} effort` : null) : (o.variant ?? 'default effort')
  return [o.model, effort, o.agent ? `${o.agent} agent` : null].filter(Boolean).join(' · ')
}
