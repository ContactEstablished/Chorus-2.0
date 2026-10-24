<script setup lang="ts">
/**
 * One session's terminal. On mount it attaches to the back end, which sends the saved scrollback
 * and then live output over one channel; on unmount it detaches and the session keeps running.
 * Keyed by session id in the parent, so switching sessions mounts a fresh terminal.
 */
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { Channel } from '@tauri-apps/api/core'
import TerminalView from '../components/TerminalView.vue'
import { registerTerminal } from '../components/terminals'
import { commands } from '../ipc'

// `autofocus` is off where several terminals mount at once (the Wall), so none takes focus.
const props = withDefaults(defineProps<{ sessionId: number; live: boolean; autofocus?: boolean; searchable?: boolean }>(), { autofocus: true, searchable: false })
const emit = defineEmits<{ error: [message: string]; attached: [attached: boolean]; size: [cols: number, rows: number]; search: [] }>()

const term = ref<InstanceType<typeof TerminalView> | null>(null)
let viewId: number | null = null
let disposed = false
const unregister = registerTerminal(props.sessionId, {
  screenText: () => term.value?.screenText() ?? '',
  bufferText: () => term.value?.bufferText() ?? '',
  focus: () => term.value?.focus(),
})

onMounted(async () => {
  const output = new Channel<ArrayBuffer>()
  output.onmessage = (buf) => term.value?.write(new Uint8Array(buf))
  const res = await commands.sessionAttach(props.sessionId, output)
  if (res.status === 'error') {
    emit('error', res.error)
    return
  }
  if (disposed) {
    void commands.sessionDetach(props.sessionId, res.data)
    return
  }
  viewId = res.data
  emit('attached', true)
  // The PTY follows this view's size from now on.
  const { cols, rows } = term.value?.size() ?? { cols: 80, rows: 24 }
  void commands.sessionResize(props.sessionId, cols, rows)
  emit('size', cols, rows)
  if (props.autofocus) term.value?.focus()
})

onBeforeUnmount(() => {
  disposed = true
  unregister()
  if (viewId !== null) void commands.sessionDetach(props.sessionId, viewId)
})

async function input(data: string): Promise<void> {
  if (!props.live) return
  const res = await commands.sessionWrite(props.sessionId, data)
  if (res.status === 'error') emit('error', res.error)
}

function resize(cols: number, rows: number): void {
  void commands.sessionResize(props.sessionId, cols, rows)
  emit('size', cols, rows)
}

function screenText(): string {
  return term.value?.screenText() ?? ''
}

defineExpose({
  screenText,
  bufferText: () => term.value?.bufferText() ?? '',
  focus: () => term.value?.focus(),
  find: (text: string, backwards = false, caseSensitive = false) => term.value?.find(text, backwards, caseSensitive) ?? false,
  clearSearch: () => term.value?.clearSearch(),
  results: () => term.value?.results ?? null,
})
</script>

<template>
  <TerminalView ref="term" :searchable="searchable" @input="input" @resize="resize" @search="emit('search')" />
</template>
