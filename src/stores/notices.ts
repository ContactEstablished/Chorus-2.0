import { defineStore } from 'pinia'
import { ref } from 'vue'

export type NoticeKind = 'info' | 'success' | 'error'

export interface Notice {
  id: number
  kind: NoticeKind
  text: string
  /** The underlying error or extra detail, shown smaller. */
  detail: string | null
}

/** How long a notice stays, in ms. Errors stay until dismissed (fail visible). */
const LIFETIME: Record<NoticeKind, number | null> = { info: 6000, success: 4000, error: null }
const MAX_SHOWN = 5

/** In-app notices: short messages in the corner of the window. */
export const useNoticesStore = defineStore('notices', () => {
  const list = ref<Notice[]>([])
  let next = 1

  function push(kind: NoticeKind, text: string, detail: unknown = null): number {
    const id = next++
    const d = detail === null || detail === undefined ? null : detail instanceof Error ? detail.message : String(detail)
    list.value = [...list.value, { id, kind, text, detail: d }].slice(-MAX_SHOWN)
    const life = LIFETIME[kind]
    if (life !== null) setTimeout(() => dismiss(id), life)
    return id
  }

  function dismiss(id: number): void {
    list.value = list.value.filter((n) => n.id !== id)
  }

  return {
    list,
    info: (text: string, detail?: unknown) => push('info', text, detail),
    success: (text: string, detail?: unknown) => push('success', text, detail),
    error: (text: string, detail?: unknown) => push('error', text, detail),
    dismiss,
  }
})
