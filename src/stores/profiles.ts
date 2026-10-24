import { defineStore } from 'pinia'
import { ref } from 'vue'
import { commands, unwrap, type ProfileSave, type ProfileSummary } from '../ipc'

/** Saved launch profiles: named settings the launch form can be filled from (Bots in P8). */
export const useProfilesStore = defineStore('profiles', () => {
  const list = ref<ProfileSummary[]>([])
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
    const got = await attempt(() => unwrap(commands.profilesList()))
    if (got) list.value = got
  }

  async function save(input: ProfileSave): Promise<ProfileSummary | null> {
    const saved = await attempt(() => unwrap(commands.profileSave(input)))
    if (saved) list.value = [...list.value.filter((p) => p.id !== saved.id), saved].sort((a, b) => a.name.localeCompare(b.name))
    return saved
  }

  async function remove(id: number): Promise<void> {
    if ((await attempt(() => unwrap(commands.profileDelete(id)))) !== null) list.value = list.value.filter((p) => p.id !== id)
  }

  /** Archives a Bot (P8): it leaves New session and can be restored on the Bots screen, where its
   * instructions and skills are kept. */
  async function archive(id: number): Promise<void> {
    if ((await attempt(() => unwrap(commands.botSetArchived(id, true)))) !== null) list.value = list.value.filter((p) => p.id !== id)
  }

  return { list, error, load, save, remove, archive }
})
