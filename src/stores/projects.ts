import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands, unwrap, type Project, type ProjectInput, type ProjectState } from '../ipc'
import { useNoticesStore } from './notices'

/** Projects as the back end keeps them, in rail order. Every change returns the whole list. */
export const useProjectsStore = defineStore('projects', () => {
  const list = ref<Project[]>([])

  const byId = computed(() => new Map(list.value.map((p) => [p.id, p])))
  /** In the rail: active ones only. */
  const active = computed(() => list.value.filter((p) => p.state === 'active'))
  /** Offered for new sessions and filters: not archived. */
  const usable = computed(() => list.value.filter((p) => p.state !== 'archived'))

  async function run(what: string, request: () => Promise<Project[]>): Promise<boolean> {
    try {
      list.value = await request()
      return true
    } catch (e) {
      useNoticesStore().error(what, e)
      return false
    }
  }

  async function load(): Promise<void> {
    await run("Couldn't load projects.", () => unwrap(commands.projectsList()))
  }

  const save = (id: number | null, input: ProjectInput) =>
    run("Couldn't save the project.", () => unwrap(commands.projectSave(id, input)))
  const setState = (id: number, state: ProjectState) =>
    run("Couldn't change the project.", () => unwrap(commands.projectSetState(id, state)))
  const reorder = (ids: number[]) => run("Couldn't reorder projects.", () => unwrap(commands.projectsReorder(ids)))
  const remove = (id: number) => run("Couldn't delete the project.", () => unwrap(commands.projectDelete(id)))

  return { list, byId, active, usable, load, save, setState, reorder, remove }
})
