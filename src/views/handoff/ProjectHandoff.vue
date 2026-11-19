<script setup lang="ts">
// P18: a project's hand-off cards on its Overview page: the return card when MausBot handed it
// back, the banner while it's with MausBot, the hand-off note, and the Hand to MausBot panel.
import { computed, onBeforeUnmount, onMounted, watch } from 'vue'
import type { Project } from '../../ipc'
import { useHandoffStore } from '../../stores/handoff'
import BatonBanner from './BatonBanner.vue'
import BranchesCard from './BranchesCard.vue'
import HandoffNote from './HandoffNote.vue'
import HandToMausBot from './HandToMausBot.vue'
import ReturnCard from './ReturnCard.vue'

const props = defineProps<{ project: Project }>()
const store = useHandoffStore()

const view = computed(() => store.views.get(props.project.id) ?? null)
const error = computed(() => store.errors.get(props.project.id) ?? null)
const handOpen = computed(() => store.openHandTo === props.project.id)

onMounted(() => {
  void store.init()
})
// The Hand to MausBot panel belongs to this visit: leaving the project closes it.
onBeforeUnmount(() => {
  if (store.openHandTo === props.project.id) store.openHandTo = null
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
    <template v-if="view">
      <ReturnCard v-if="view.returned" :project="project" :returned="view.returned" />
      <BatonBanner v-if="view.active" :project-id="project.id" :active="view.active" />
      <HandoffNote :view="view">
        <template #actions>
          <button v-if="!view.active && view.hasFolder && !handOpen" class="quiet" @click="store.openHandTo = project.id">Hand to MausBot</button>
        </template>
      </HandoffNote>
      <HandToMausBot v-if="handOpen && !view.active" :project="project" @close="store.openHandTo = null" />
      <BranchesCard v-if="view.hasFolder" :project-id="project.id" />
    </template>
    <p v-if="error && view && !handOpen" class="warn" role="alert">{{ error }}</p>
  </div>
</template>

<style scoped>
.handoff { display: grid; gap: 10px; margin: 0 0 18px; }
.warn { margin: 0; font-size: 12.5px; color: var(--status-needs); overflow-wrap: anywhere; }
</style>
