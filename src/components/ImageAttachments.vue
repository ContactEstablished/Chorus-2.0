<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from 'vue'
import type { ComposerImage } from '../ipc'
import { imagePreview } from './composerImages'
import Icon from './Icon.vue'

const props = defineProps<{ sessionId: number; images: ComposerImage[]; removable?: boolean; disabled?: boolean }>()
const emit = defineEmits<{ remove: [id: number] }>()
const thumbnails = ref<Record<number, string>>({})
const errors = ref<Record<number, string>>({})
const dialog = ref<HTMLDialogElement | null>(null)
const selected = ref<ComposerImage | null>(null)
const full = ref('')
const previewError = ref('')
let generation = 0
let previewGeneration = 0
let opener: HTMLElement | null = null
watch(() => `${props.sessionId}:${props.images.map(i => i.id).join(',')}`, async () => {
  const current = ++generation
  const session = props.sessionId
  thumbnails.value = {}; errors.value = {}
  close()
  // Decode one image at a time to bound temporary memory.
  for (const image of props.images) {
    if (current !== generation) return
    try {
      const src = await imagePreview(session, image.id, true)
      if (current === generation) thumbnails.value[image.id] = src
    } catch (e) { if (current === generation) errors.value[image.id] = String(e) }
  }
}, { immediate: true })
async function preview(image: ComposerImage) {
  const current = ++previewGeneration
  opener = document.activeElement instanceof HTMLElement ? document.activeElement : null
  selected.value = image; full.value = ''; previewError.value = ''
  await nextTick()
  if (current !== previewGeneration) return
  dialog.value?.showModal()
  try {
    const src = await imagePreview(props.sessionId, image.id)
    if (current === previewGeneration) full.value = src
  } catch (e) { if (current === previewGeneration) previewError.value = String(e) }
}
function close() {
  previewGeneration++
  dialog.value?.close()
  selected.value = null; full.value = ''
  if (opener?.isConnected) opener.focus()
  opener = null
}
onBeforeUnmount(() => { generation++; close() })
</script>

<template>
  <ul class="attachments" aria-label="Attached images">
    <li v-for="(image, index) in images" :key="image.id" class="attachment">
      <button class="thumbnail" :aria-label="`Preview image ${index + 1}: ${image.name}`" @click="preview(image)">
        <img v-if="thumbnails[image.id]" :src="thumbnails[image.id]" :alt="image.name">
        <span v-else>{{ errors[image.id] ? 'Preview unavailable' : 'Loading…' }}</span>
        <span class="zoom"><Icon name="search" :size="16" /></span>
      </button>
      <button v-if="removable" class="remove" :disabled="disabled" :aria-label="`Remove image ${index + 1}: ${image.name}`" :title="`Remove ${image.name}`" @click="emit('remove', image.id)"><Icon name="close" :size="14" /></button>
      <span class="name" :title="image.name">{{ index + 1 }}. {{ image.name }}</span>
    </li>
  </ul>
  <Teleport to="body">
    <dialog ref="dialog" class="image-preview" aria-label="Image preview" @cancel.prevent="close" @click="($event.target === dialog) && close()">
      <div class="preview-body">
        <header><h2>{{ selected?.name }}</h2><button aria-label="Close image preview" autofocus @click="close"><Icon name="close" /></button></header>
        <p v-if="previewError" role="alert">{{ previewError }}</p>
        <img v-else-if="full" :src="full" :alt="selected?.name">
        <p v-else>Loading image…</p>
      </div>
    </dialog>
  </Teleport>
</template>

<style scoped>
.attachments { display: flex; flex-wrap: wrap; gap: 12px; list-style: none; padding: 0; margin: 14px 0; }
.attachment { position: relative; width: 100px; margin: 0; }
.thumbnail { position: relative; display: grid; place-items: center; width: 100px; height: 82px; padding: 4px; border: 1px solid var(--border-strong); border-radius: 9px; background: var(--surface); overflow: hidden; }
.thumbnail img { width: 100%; height: 100%; object-fit: contain; }
.thumbnail > span { font-size: 10px; }
.zoom { position: absolute; bottom: 3px; right: 3px; padding: 3px; border-radius: 5px; background: var(--surface-raised); color: var(--text); display: flex; }
.remove { position: absolute; top: -6px; right: -6px; width: 24px; height: 24px; padding: 3px; border-radius: 50%; background: var(--surface-raised); display: grid; place-items: center; }
.name { display: block; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-size: 11px; color: var(--text-muted); margin-top: 5px; }
.image-preview { width: min(680px, calc(100vw - 48px)); max-height: min(640px, calc(100dvh - 64px)); padding: 0; border: 1px solid var(--border-strong); border-radius: 14px; background: var(--surface-raised); color: var(--text); box-shadow: 0 24px 80px #0008; }
.image-preview::backdrop { background: #0009; }
.preview-body { padding: 18px; }
header { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-bottom: 14px; }
h2 { margin: 0; font-size: 14px; overflow-wrap: anywhere; }
header button { display: flex; padding: 6px; }
.preview-body > img { display: block; width: 100%; height: min(480px, calc(100dvh - 180px)); object-fit: contain; }
</style>
