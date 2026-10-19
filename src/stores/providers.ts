import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands, unwrap, type CatalogEntry, type ProviderSave, type ProviderSummary } from '../ipc'

/**
 * Providers (OpenRouter, gateways, Jev): which keys they use, their last connection test, and
 * their model catalogs and shortlists. Keys themselves never come to the front end.
 */
export const useProvidersStore = defineStore('providers', () => {
  const list = ref<ProviderSummary[]>([])
  const error = ref<string | null>(null)
  /** The provider an action is running for (a test or a catalog refresh can take seconds). */
  const busyId = ref<number | null>(null)
  const byId = computed(() => new Map(list.value.map((p) => [p.id, p])))

  async function attempt<T>(run: () => Promise<T>): Promise<T | null> {
    error.value = null
    try {
      return await run()
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      return null
    }
  }

  function put(p: ProviderSummary): void {
    const rest = list.value.filter((x) => x.id !== p.id)
    list.value = [...rest, p].sort((a, b) => a.name.localeCompare(b.name))
  }

  async function load(): Promise<void> {
    const got = await attempt(() => unwrap(commands.providersList()))
    if (got) list.value = got
  }

  async function save(input: ProviderSave): Promise<ProviderSummary | null> {
    const saved = await attempt(() => unwrap(commands.providerSave(input)))
    if (saved) put(saved)
    return saved
  }

  async function remove(id: number): Promise<void> {
    if ((await attempt(() => unwrap(commands.providerDelete(id)))) !== null) {
      list.value = list.value.filter((p) => p.id !== id)
    }
  }

  async function busy(id: number, run: () => Promise<ProviderSummary>): Promise<void> {
    busyId.value = id
    const got = await attempt(run)
    busyId.value = null
    if (got) put(got)
  }

  const test = (id: number) => busy(id, () => unwrap(commands.providerTest(id)))
  const refresh = (id: number) => busy(id, () => unwrap(commands.providerRefreshCatalog(id)))

  async function catalog(id: number): Promise<CatalogEntry[]> {
    return (await attempt(() => unwrap(commands.providerCatalog(id)))) ?? []
  }

  async function setShortlist(id: number, models: string[]): Promise<boolean> {
    const got = await attempt(() => unwrap(commands.providerSetShortlist(id, models)))
    if (got) put(got)
    return got !== null
  }

  return { list, byId, error, busyId, load, save, remove, test, refresh, catalog, setShortlist }
})
