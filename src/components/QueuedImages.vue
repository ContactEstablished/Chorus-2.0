<script setup lang="ts">
import { ref } from 'vue'
import type { ComposerImage } from '../ipc'
import ImageAttachments from './ImageAttachments.vue'
defineProps<{ sessionId: number; images: ComposerImage[] }>()
const open = ref(false)
</script>
<template>
  <details v-if="images.length" @toggle="open = ($event.target as HTMLDetailsElement).open">
    <summary>{{ images.length }} attached {{ images.length === 1 ? 'image' : 'images' }}</summary>
    <ImageAttachments v-if="open" :session-id="sessionId" :images="images" />
  </details>
</template>
<style scoped>summary { cursor: pointer; font-size: 12px; margin: 8px 0; }</style>
