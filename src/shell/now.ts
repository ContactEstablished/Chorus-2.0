import { onScopeDispose, ref, type Ref } from 'vue'

/** The time in seconds, ticking every `everyMs`, for "updated 4m ago" texts. */
export function useNow(everyMs = 10_000): Ref<number> {
  const now = ref(Math.floor(Date.now() / 1000))
  const timer = setInterval(() => (now.value = Math.floor(Date.now() / 1000)), everyMs)
  onScopeDispose(() => clearInterval(timer))
  return now
}
