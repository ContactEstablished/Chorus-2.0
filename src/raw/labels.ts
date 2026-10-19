import type { AgentInfo, Effort, Permission, SessionSummary, VersionFit } from '../ipc'

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
export function optionsText(s: SessionSummary): string {
  const parts: string[] = []
  if (s.options.effort) parts.push(`${EFFORT_LABELS[s.options.effort]} effort`)
  if (s.options.permission) parts.push(PERMISSION_LABELS[s.options.permission])
  if (s.options.model) parts.push(s.options.model)
  return parts.join(' · ')
}

/** What the newest turn really ran with, read back from the agent: "glm-5.3-flash-free · high · plan agent". */
export function observedText(s: SessionSummary): string | null {
  const o = s.observed
  if (!o) return null
  return [o.model, o.variant ?? 'default effort', o.agent ? `${o.agent} agent` : null].filter(Boolean).join(' · ')
}
