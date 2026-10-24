import { defineStore } from 'pinia'
import { ref } from 'vue'
import { commands, unwrap, type Bot } from '../ipc'
import { useNoticesStore } from './notices'
import { useViewStore } from './view'

/** Bots (P8): the launch profiles, grown. Start session opens New session filled from one. */
export const useBotsStore = defineStore('bots', () => {
  const list = ref<Bot[]>([])
  /** The Bot New session applies when it opens, then forgets. */
  const startWith = ref<Bot | null>(null)

  async function load(): Promise<void> {
    try {
      list.value = await unwrap(commands.botsList())
    } catch (e) {
      useNoticesStore().error("Couldn't load Bots.", e)
    }
  }

  function start(bot: Bot): void {
    startWith.value = bot
    useViewStore().newSessionOpen = true
  }

  return { list, startWith, load, start }
})
