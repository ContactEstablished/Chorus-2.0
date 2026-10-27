<script setup lang="ts">
/**
 * Thumbnails for image files an agent named by full path, shown under the message so they can be
 * previewed, copied as an image, or copied as a path. The back end only reads absolute image paths.
 */
import { computed, ref, watch } from 'vue'
import { commands, unwrap } from '../ipc'
import { useNoticesStore } from '../stores/notices'
import { dataUrl, loadImage } from './composerImages'
import { extractImagePaths } from './imagePaths'

const props = defineProps<{ text: string }>()
const notices = useNoticesStore()
const paths = computed(() => extractImagePaths(props.text))
const items = ref<{ path: string; src: string; error: string }[]>([])
const dialog = ref<HTMLDialogElement | null>(null)
const selected = ref('')
const full = ref('')
const fullError = ref('')
let generation = 0

const name = (path: string) => path.split(/[\\/]/).pop() ?? path
function mime(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase()
  return ext === 'jpg' || ext === 'jpeg' ? 'image/jpeg' : ext === 'webp' ? 'image/webp' : ext === 'gif' ? 'image/gif' : 'image/png'
}
async function read(path: string): Promise<string> {
  return dataUrl(await unwrap(commands.localImageRead(path))).replace('image/png', mime(path))
}
async function thumbnail(path: string): Promise<string> {
  const image = await loadImage(await read(path))
  const scale = Math.min(1, 320 / Math.max(image.naturalWidth, image.naturalHeight))
  const canvas = document.createElement('canvas')
  canvas.width = Math.max(1, Math.round(image.naturalWidth * scale))
  canvas.height = Math.max(1, Math.round(image.naturalHeight * scale))
  canvas.getContext('2d')!.drawImage(image, 0, 0, canvas.width, canvas.height)
  return canvas.toDataURL('image/png')
}
watch(paths, async (list) => {
  const current = ++generation
  items.value = list.map(path => ({ path, src: '', error: '' }))
  for (const item of items.value) {
    try { const src = await thumbnail(item.path); if (current === generation) item.src = src }
    catch (e) { if (current === generation) item.error = String(e) }
  }
}, { immediate: true })

async function copyPath(path: string) {
  try { await navigator.clipboard.writeText(path); notices.success('Path copied.') }
  catch (e) { notices.error('Could not copy the path.', e) }
}
/** The clipboard takes PNG, so re-encode the full-size file whatever its type. */
async function copyImage(path: string) {
  try {
    const image = await loadImage(await read(path))
    const canvas = document.createElement('canvas')
    canvas.width = image.naturalWidth; canvas.height = image.naturalHeight
    canvas.getContext('2d')!.drawImage(image, 0, 0)
    const blob = await new Promise<Blob | null>(r => canvas.toBlob(r, 'image/png'))
    if (!blob) throw new Error('The image could not be converted.')
    await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })])
    notices.success('Image copied.')
  } catch (e) { notices.error('Could not copy the image.', e) }
}
async function preview(path: string) {
  selected.value = path; full.value = ''; fullError.value = ''
  dialog.value?.showModal()
  try {
    const src = await read(path)
    if (selected.value === path) full.value = src
  } catch (e) { if (selected.value === path) fullError.value = String(e) }
}
function close() { dialog.value?.close(); selected.value = ''; full.value = '' }
</script>

<template>
  <ul v-if="items.length" class="linked" aria-label="Images mentioned in this message">
    <li v-for="item in items" :key="item.path">
      <button class="thumb" :title="`Preview ${item.path}`" :aria-label="`Preview image ${name(item.path)}`" :disabled="!item.src" @click="preview(item.path)">
        <img v-if="item.src" :src="item.src" :alt="name(item.path)">
        <span v-else>{{ item.error ? 'Preview unavailable' : 'Loading…' }}</span>
      </button>
      <div class="meta">
        <span class="file" :title="item.path">{{ name(item.path) }}</span>
        <span v-if="item.error" class="err">{{ item.error }}</span>
        <span v-else class="path" :title="item.path">{{ item.path }}</span>
        <span class="actions">
          <button class="quiet" @click="copyPath(item.path)">Copy path</button>
          <button class="quiet" :disabled="!item.src" @click="copyImage(item.path)">Copy image</button>
        </span>
      </div>
    </li>
  </ul>
  <Teleport v-if="items.length" to="body">
    <dialog ref="dialog" class="linked-preview" aria-label="Image preview" @cancel.prevent="close" @click="($event.target === dialog) && close()">
      <div class="body">
        <header><h2>{{ selected }}</h2><button aria-label="Close image preview" @click="close">Close</button></header>
        <p v-if="fullError" role="alert">{{ fullError }}</p>
        <img v-else-if="full" :src="full" :alt="name(selected)">
        <p v-else>Loading image…</p>
      </div>
    </dialog>
  </Teleport>
</template>

<style scoped>
.linked { display: grid; gap: 10px; list-style: none; padding: 0; margin: 12px 0 0; }
.linked li { display: flex; gap: 12px; align-items: center; padding: 10px; border: 1px solid var(--border-strong); border-radius: 10px; background: var(--surface); min-width: 0; }
.thumb { flex: none; display: grid; place-items: center; width: 140px; height: 96px; padding: 4px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); overflow: hidden; font-size: 11px; }
.thumb img { width: 100%; height: 100%; object-fit: contain; }
.meta { display: grid; gap: 3px; min-width: 0; }
.file { font-weight: 600; font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.path, .err { font: 11px var(--font-mono); color: var(--text-muted); overflow-wrap: anywhere; }
.err { color: var(--status-failed); }
.actions { display: flex; gap: 6px; margin-top: 4px; }
.actions button { font-size: 12px; padding: 3px 9px; }
.linked-preview { width: min(900px, calc(100vw - 48px)); max-height: calc(100dvh - 64px); padding: 0; border: 1px solid var(--border-strong); border-radius: 14px; background: var(--surface-raised); color: var(--text); }
.linked-preview::backdrop { background: #0009; }
.body { padding: 18px; }
header { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-bottom: 14px; }
h2 { margin: 0; font-size: 13px; font-family: var(--font-mono); overflow-wrap: anywhere; }
.body > img { display: block; width: 100%; max-height: calc(100dvh - 180px); object-fit: contain; }
</style>
