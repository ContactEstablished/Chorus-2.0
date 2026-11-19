<script setup lang="ts">
// P18: a project's hand-off cards on its Overview page. 18-1 shows the note; 18-2 and 18-3 add
// the baton, the return card and the branches here.
import { computed, onMounted, watch } from 'vue'
import type { Project } from '../../ipc'
import { useHandoffStore } from '../../stores/handoff'
import HandoffNote from './HandoffNote.vue'

const props = defineProps<{ project: Project }>()
const store = useHandoffStore()

const view = computed(() => store.views.get(props.project.id) ?? null)
const error = computed(() => store.errors.get(props.project.id) ?? null)

onMounted(() => {
  void store.init()
})
watch(
  () => props.project.id,
  (id) => void store.load(id),
  { immediate: true },
)
</script>

<template>
  <div class="handoff">
    <p v-if="error && !view" class="warn" role="alert">The hand-off note couldn’t be read: {{ error }}</p>
    <HandoffNote v-if="view" :view="view" />
    <p v-if="error && view" class="warn" role="alert">{{ error }}</p>
  </div>
</template>

<style scoped>
.handoff { display: grid; gap: 10px; margin: 0 0 18px; }
.warn { margin: 0; font-size: 12.5px; color: var(--status-needs); }
</style>
