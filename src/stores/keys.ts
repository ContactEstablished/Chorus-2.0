import { defineStore } from 'pinia'
import { ref } from 'vue'
import { commands, unwrap, type CredentialSummary } from '../ipc'

/**
 * The vault's keys (labels and fingerprints only: a key never comes back from the back end) and
 * whether an agent-lock PIN is set.
 */
export const useKeysStore = defineStore('keys', () => {
  const keys = ref<CredentialSummary[]>([])
  const pinSet = ref(false)
  const error = ref<string | null>(null)

  async function attempt<T>(run: () => Promise<T>): Promise<T | null> {
    error.value = null
    try {
      return await run()
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      return null
    }
  }

  async function load(): Promise<void> {
    await attempt(async () => {
      keys.value = await unwrap(commands.vaultList())
      pinSet.value = await unwrap(commands.lockPinStatus())
    })
  }

  /** Saves a key. The caller should clear its input whatever happens. */
  async function add(label: string, secret: string): Promise<boolean> {
    const saved = await attempt(() => unwrap(commands.vaultAdd(label, secret)))
    if (saved) keys.value = [...keys.value, saved].sort((a, b) => a.label.localeCompare(b.label))
    return saved !== null
  }

  async function remove(id: number): Promise<void> {
    if ((await attempt(() => unwrap(commands.vaultDelete(id)))) !== null) {
      keys.value = keys.value.filter((k) => k.id !== id)
    }
  }

  async function setPin(pin: string): Promise<boolean> {
    const ok = await attempt(async () => (await unwrap(commands.lockPinSet(pin)), true))
    if (ok) pinSet.value = true
    return ok === true
  }

  async function clearPin(): Promise<void> {
    if (await attempt(async () => (await unwrap(commands.lockPinClear()), true))) pinSet.value = false
  }

  return { keys, pinSet, error, load, add, remove, setPin, clearPin }
})
