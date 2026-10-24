<script setup lang="ts">
import { computed, ref } from 'vue'
import { openUrl } from '@tauri-apps/plugin-opener'
import { renderMessage, webUrl } from './messageMarkdown'
import { useNoticesStore } from '../stores/notices'

const props = defineProps<{ text: string }>()
const html = computed(() => renderMessage(props.text))
const notices = useNoticesStore()
const source = ref(false)

async function copy(text: string, label: string): Promise<void> {
  try { await navigator.clipboard.writeText(text); notices.success(`${label} copied.`) }
  catch (e) { notices.error(`Could not copy ${label.toLowerCase()}.`, e) }
}

function messageAction(event: MouseEvent): void {
  const target = event.target instanceof Element ? event.target : null
  const button = target?.closest<HTMLButtonElement>('button[data-message-action]')
  if (!button) { followLink(event); return }
  event.preventDefault()
  if (button.dataset.messageAction === 'copy-code') {
    void copy(button.closest('.code-card')?.querySelector('code')?.textContent ?? '', 'Code')
  } else if (button.dataset.messageAction === 'image') {
    const card = button.closest<HTMLElement>('.image-card')
    const src = webUrl(card?.dataset.imageUrl ?? '')
    if (!card || !src?.startsWith('https://')) return
    const existing = card.querySelector('img')
    const status = card.querySelector('.image-status')!
    if (existing) { existing.remove(); button.textContent = 'Show image'; status.textContent = ''; return }
    const img = document.createElement('img')
    img.alt = card.dataset.imageAlt ?? 'Image'
    img.referrerPolicy = 'no-referrer'
    img.onload = () => { status.textContent = ''; button.disabled = false; button.textContent = 'Hide image' }
    img.onerror = () => { img.remove(); status.textContent = 'Image could not load. You can retry.'; button.disabled = false; button.textContent = 'Retry image' }
    button.disabled = true
    status.textContent = 'Loading image…'
    card.append(img)
    img.src = src
  }
}

function followLink(event: MouseEvent): void {
  const link = event.target instanceof Element ? event.target.closest('a') : null
  if (!link) return
  event.preventDefault()
  if (event.type === 'auxclick' && event.button !== 1) return
  const href = link.getAttribute('href')
  const url = href && webUrl(href)
  if (url) {
    void openUrl(url).catch((e: unknown) => notices.error('Could not open that link in your browser.', e))
  }
}
</script>

<template>
  <div class="message-reader">
    <header class="message-toolbar">
      <div class="message-label"><slot /></div>
      <div class="message-actions">
        <button class="quiet" title="Copy original message" @click="copy(text, 'Message')">Copy</button>
        <button class="quiet" :aria-pressed="source" :title="source ? 'Show formatted message' : 'Show original message'" @click="source = !source">{{ source ? 'Formatted' : 'Source' }}</button>
      </div>
    </header>
    <pre v-if="source" class="message-source">{{ text }}</pre>
    <!-- Only the restricted Markdown renderer produces this HTML; source HTML is escaped. -->
    <div v-else class="message-content" @click="messageAction" @auxclick="followLink" v-html="html" />
  </div>
</template>

<style scoped>
.message-reader { min-width: 0; }
.message-toolbar { display: flex; gap: 12px; align-items: center; justify-content: space-between; margin: -4px 0 14px; }
.message-label { font-size: 12px; letter-spacing: 0.04em; font-weight: 600; }
.message-actions { display: flex; gap: 4px; }
.message-actions button { font-size: 11px; padding: 3px 7px; color: var(--text-muted); }
.message-source { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; font: 12px/1.7 var(--font-mono); color: var(--text-2); }
.message-content {
  color: color-mix(in srgb, var(--text) 78%, var(--text-2));
  font-size: 14px;
  line-height: 1.8;
  overflow-wrap: anywhere;
  min-width: 0;
}
.message-content :deep(> :first-child) { margin-top: 0; }
.message-content :deep(> :last-child) { margin-bottom: 0; }
.message-content :deep(p) { margin: 0.85em 0; }
.message-content :deep(h1), .message-content :deep(h2), .message-content :deep(h3),
.message-content :deep(h4), .message-content :deep(h5), .message-content :deep(h6) {
  color: var(--text);
  font-family: var(--font-heading);
  font-weight: 600;
  line-height: 1.35;
  margin: 1.5em 0 0.65em;
}
.message-content :deep(h1) { font-size: 23px; }
.message-content :deep(h2) { font-size: 20px; }
.message-content :deep(h3) { font-size: 17px; }
.message-content :deep(h4), .message-content :deep(h5), .message-content :deep(h6) { font-size: 15px; }
.message-content :deep(strong) { color: var(--text); font-weight: 650; }
.message-content :deep(em) { color: var(--tag-lavender); }
.message-content :deep(s) { color: var(--text-muted); }
.message-content :deep(a) {
  color: var(--accent);
  text-decoration-color: var(--accent-line);
  text-underline-offset: 3px;
  border-radius: 2px;
}
.message-content :deep(a:hover) { color: var(--accent-hover); text-decoration-color: currentColor; }
.message-content :deep(a:focus-visible) { outline: 2px solid var(--accent); outline-offset: 3px; }
.message-content :deep(ul), .message-content :deep(ol) { margin: 0.85em 0; padding-left: 1.7em; }
.message-content :deep(li) { padding-left: 0.35em; margin: 0.35em 0; }
.message-content :deep(li::marker) { color: var(--accent); font-weight: 600; }
.message-content :deep(li > ul), .message-content :deep(li > ol), .message-content :deep(li > p) { margin: 0.3em 0; }
.message-content :deep(blockquote) {
  margin: 1.2em 0;
  padding: 10px 18px;
  border-left: 3px solid var(--accent-line);
  border-radius: 0 9px 9px 0;
  background: color-mix(in srgb, var(--accent) 5%, transparent);
  color: var(--text-2);
}
.message-content :deep(blockquote > :first-child) { margin-top: 0; }
.message-content :deep(blockquote > :last-child) { margin-bottom: 0; }
.message-content :deep(code) {
  font-family: var(--font-mono);
  font-size: 0.86em;
  padding: 2px 6px;
  border: 1px solid color-mix(in srgb, var(--tag-lavender) 15%, transparent);
  border-radius: 5px;
  background: color-mix(in srgb, var(--tag-lavender) 7%, transparent);
  color: var(--tag-lavender);
  box-decoration-break: clone;
}
.message-content :deep(pre) {
  margin: 1.1em 0;
  padding: 16px 18px;
  max-height: 440px;
  overflow: auto;
  white-space: pre;
  overflow-wrap: normal;
  border: 1px solid var(--border-strong);
  border-radius: 10px;
  background: var(--bg);
  line-height: 1.65;
  tab-size: 2;
}
.message-content :deep(pre code) { padding: 0; border: 0; background: none; color: var(--text-2); }
.message-content :deep(.message-table) {
  overflow-x: auto;
  margin: 1.25em 0;
  border: 1px solid var(--border-strong);
  border-radius: 10px;
}
.message-content :deep(.message-table:focus-visible) { outline: 2px solid var(--accent-line); outline-offset: 2px; }
.message-content :deep(table) { width: 100%; border-collapse: collapse; font-size: 13.5px; line-height: 1.65; }
.message-content :deep(th), .message-content :deep(td) { padding: 10px 14px; text-align: left; min-width: 100px; }
.message-content :deep(th) { color: var(--accent); background: color-mix(in srgb, var(--accent) 7%, var(--surface)); font-weight: 600; }
.message-content :deep(td) { border-top: 1px solid var(--border); }
.message-content :deep(tbody tr:nth-child(even)) { background: color-mix(in srgb, var(--text) 2%, transparent); }
.message-content :deep(tbody tr:hover) { background: color-mix(in srgb, var(--accent) 4%, transparent); }
.message-content :deep(hr) { border: 0; height: 1px; background: var(--border-strong); margin: 1.6em 0; }
.message-content :deep(mark) { color: var(--text); background: color-mix(in srgb, var(--status-needs) 18%, transparent); border-radius: 4px; padding: 1px 4px; }
.message-content :deep(.callout) { --callout: var(--accent); border-left-color: var(--callout); background: color-mix(in srgb, var(--callout) 6%, transparent); }
.message-content :deep(.callout-tip) { --callout: var(--status-ready); }
.message-content :deep(.callout-important) { --callout: var(--tag-lavender); }
.message-content :deep(.callout-warning) { --callout: var(--status-needs); }
.message-content :deep(.callout-caution) { --callout: var(--status-failed); }
.message-content :deep(.callout-title) { color: var(--callout); font-weight: 650; font-size: 12px; letter-spacing: 0.04em; margin-bottom: 4px; }
.message-content :deep(.callout > p) { margin: 0; }
.message-content :deep(.task-item) { list-style: none; position: relative; }
.message-content :deep(.task-marker) { position: absolute; left: -1.5em; color: var(--text-muted); }
.message-content :deep(.task-done > .task-marker) { color: var(--status-ready); }
.message-content :deep(.code-card) { overflow: hidden; border: 1px solid var(--border-strong); border-radius: 10px; margin: 1.2em 0; }
.message-content :deep(.code-card header) { display: flex; align-items: center; justify-content: space-between; padding: 7px 14px; background: var(--surface); color: var(--tag-lavender); font: 11px var(--font-mono); }
.message-content :deep(.code-card button), .message-content :deep(.image-card button) { font: 12px var(--font-ui); padding: 3px 9px; background: transparent; color: var(--text-2); }
.message-content :deep(.code-card pre) { margin: 0; border: 0; border-top: 1px solid var(--border); border-radius: 0; }
.message-content :deep(.image-card) { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; padding: 14px; margin: 12px 0; background: var(--surface); border: 1px solid var(--border-strong); border-radius: 10px; }
.message-content :deep(.image-caption) { color: var(--text); font-weight: 600; }
.message-content :deep(.image-host), .message-content :deep(.image-status), .message-content :deep(.image-unavailable) { color: var(--text-muted); font-size: 12px; }
.message-content :deep(.image-card button) { margin-left: auto; }
.message-content :deep(.image-status:empty) { display: none; }
.message-content :deep(.image-card img) { display: block; width: 100%; max-height: 480px; object-fit: contain; border-radius: 6px; }
</style>
