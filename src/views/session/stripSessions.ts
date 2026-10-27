import type { SessionSummary } from '../../ipc'
import { byUrgency } from '../../shell/status'

/**
 * The sessions the strip offers while `currentId` is open: those in the same project, most urgent
 * first. A session in no project sees the others in no project. An unknown id gets nothing.
 */
export function stripSessions(all: SessionSummary[], currentId: number): SessionSummary[] {
  const current = all.find((s) => s.id === currentId)
  if (!current) return []
  return all.filter((s) => s.projectId === current.projectId).sort(byUrgency)
}
