import { defineStore } from 'pinia'
import { ref, watch } from 'vue'

/** Local view preferences, separate from session state and shared across views. */
export const useLayoutStore = defineStore('layout', () => {
  function preference(key: string, fallback: boolean) {
    let initial = fallback
    try {
      const saved = localStorage.getItem(key)
      if (saved !== null) initial = saved === 'true'
    } catch { /* The layout still works when storage is unavailable. */ }
    const value = ref(initial)
    watch(value, (next) => {
      try { localStorage.setItem(key, String(next)) } catch { /* Keep the in-memory choice. */ }
    })
    return value
  }

  const railCollapsed = preference('chorus.layout.railCollapsed', window.matchMedia('(max-width: 720px)').matches)
  const detailsOpen = preference('chorus.layout.detailsOpen', !window.matchMedia('(max-width: 980px)').matches)
  return { railCollapsed, detailsOpen }
})
