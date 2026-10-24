import { defineStore } from 'pinia'
import { ref } from 'vue'
import { commands, events, type FleetRoster } from '../ipc'

/**
 * Claude Code's peer fleet as Chorus reads it (P7): each session's address and the live Claude
 * sessions Chorus didn't start. The back end polls every 3 s and sends changes.
 */
export const useFleetStore = defineStore('fleet', () => {
  const roster = ref<FleetRoster>({ unreadable: null, rows: [], others: [] })
  let unlisten: (() => void) | null = null
  let started = false

  async function init(): Promise<void> {
    if (started) return
    started = true
    unlisten = await events.fleetRosterChanged.listen(({ payload }) => (roster.value = payload.roster))
    roster.value = await commands.fleetRoster()
  }

  function row(sessionId: number) {
    return roster.value.rows.find((r) => r.sessionId === sessionId) ?? null
  }

  async function acknowledge(sessionId: number): Promise<void> {
    await commands.fleetAcknowledge(sessionId)
  }

  function dispose(): void {
    unlisten?.()
    unlisten = null
    started = false
  }

  return { roster, init, row, acknowledge, dispose }
})
