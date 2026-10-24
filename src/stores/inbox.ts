import { defineStore } from 'pinia'
import { ref } from 'vue'
import { moveSelection, nextAfterReturn } from '../shell/inbox'
import type { StatusFilter } from '../shell/status'

export const useInboxStore = defineStore('inbox', () => {
  const filter = ref<StatusFilter>('all')
  const selectedId = ref<number | null>(null)
  const returning = ref<{ opened: number; ids: number[] } | null>(null)

  function move(ids: number[], direction: 1 | -1): void {
    selectedId.value = moveSelection(ids, selectedId.value, direction)
  }
  function open(id: number, ids: number[]): void {
    selectedId.value = id
    returning.value = { opened: id, ids: [...ids] }
  }
  function returned(ids: number[]): void {
    if (!returning.value) return
    selectedId.value = nextAfterReturn(returning.value.ids, returning.value.opened, ids)
    returning.value = null
  }
  return { filter, selectedId, returning, move, open, returned }
})
