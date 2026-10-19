import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands, unwrap, type AgentInfo, type AgentKind } from '../ipc'

/** Each agent's declared options and what Chorus found when it looked for the CLI. */
export const useAgentsStore = defineStore('agents', () => {
  const list = ref<AgentInfo[]>([])
  const detecting = ref(false)
  const error = ref<string | null>(null)

  const byKind = computed(() => new Map(list.value.map((a) => [a.kind, a] as [AgentKind, AgentInfo])))

  /** With `refresh`, looks for every CLI again (a second or two). */
  async function load(refresh: boolean): Promise<void> {
    error.value = null
    detecting.value = refresh
    try {
      list.value = await unwrap(commands.agentsList(refresh))
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
    } finally {
      detecting.value = false
    }
  }

  return { list, byKind, detecting, error, load }
})
